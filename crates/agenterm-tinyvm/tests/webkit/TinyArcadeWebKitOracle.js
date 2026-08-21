(function (global) {
  "use strict";

  const CORE = "tinyarcade:core/v1";
  const MAX_RENDER_BYTES = 64 * 1024;
  const MAX_AUDIO_BYTES = 16 * 1024;
  const MAX_STATE_BYTES = 256 * 1024;

  function requireCondition(condition, message) {
    if (!condition) throw new Error(message);
  }

  function runLifecycle(state, phase, callback) {
    state.phase = phase;
    state.render = [];
    state.audio = [];
    state.renderSubmitted = false;
    state.audioSubmitted = false;
    try {
      requireCondition(callback() === 0, phase + " failed");
    } finally {
      state.phase = "idle";
    }
  }

  function runTinyArcadeWebKitOracle(wasmBytes, configuration) {
    const state = {
      phase: "idle",
      buttons: 0,
      clockMs: 0,
      rng: configuration.rng >>> 0,
      render: [],
      audio: [],
      savedState: [],
      restoreState: new Uint8Array(configuration.guestState),
      renderSubmitted: false,
      audioSubmitted: false,
      stateSubmitted: false,
      stateLoaded: false,
    };
    let instance = null;

    function frameActive() {
      requireCondition(
        state.phase === "init" || state.phase === "tick",
        "frame host call outside init/tick"
      );
    }

    function memoryRange(pointer, length, maximum, label) {
      pointer |= 0;
      length |= 0;
      requireCondition(pointer >= 0 && length >= 0 && length <= maximum, label);
      const memory = instance.exports.memory;
      requireCondition(memory instanceof WebAssembly.Memory, "missing exported memory");
      const end = pointer + length;
      requireCondition(end >= pointer && end <= memory.buffer.byteLength, label);
      return new Uint8Array(memory.buffer, pointer, length);
    }

    const core = {
      input_bits: function () {
        frameActive();
        return state.buttons | 0;
      },
      clock_ms: function () {
        frameActive();
        return state.clockMs | 0;
      },
      random_u32: function () {
        frameActive();
        let value = state.rng >>> 0;
        value = (value ^ (value << 13)) >>> 0;
        value = (value ^ (value >>> 17)) >>> 0;
        value = (value ^ (value << 5)) >>> 0;
        state.rng = value;
        return value | 0;
      },
      indexed2d_version: function () {
        return 1;
      },
      indexed2d_metadata_version: function () {
        return 1;
      },
      grid3d_version: function () {
        return 1;
      },
      tones_version: function () {
        return 1;
      },
      submit_render: function (pointer, length) {
        frameActive();
        requireCondition(!state.renderSubmitted, "duplicate render submission");
        state.render = Array.from(
          memoryRange(pointer, length, MAX_RENDER_BYTES, "render bounds")
        );
        state.renderSubmitted = true;
        return 0;
      },
      submit_audio: function (pointer, length) {
        frameActive();
        requireCondition(!state.audioSubmitted, "duplicate audio submission");
        state.audio = Array.from(
          memoryRange(pointer, length, MAX_AUDIO_BYTES, "audio bounds")
        );
        state.audioSubmitted = true;
        return 0;
      },
      save_state: function (pointer, length) {
        requireCondition(state.phase === "suspend", "save outside suspend");
        requireCondition(!state.stateSubmitted, "duplicate state submission");
        state.savedState = Array.from(
          memoryRange(pointer, length, MAX_STATE_BYTES, "state bounds")
        );
        state.stateSubmitted = true;
        return 0;
      },
      load_state: function (pointer, capacity) {
        requireCondition(state.phase === "resume", "load outside resume");
        requireCondition(!state.stateLoaded, "duplicate state load");
        capacity |= 0;
        requireCondition(
          capacity >= 0 && state.restoreState.length <= capacity,
          "restore capacity"
        );
        const target = memoryRange(
          pointer,
          state.restoreState.length,
          MAX_STATE_BYTES,
          "restore bounds"
        );
        target.set(state.restoreState);
        state.stateLoaded = true;
        return state.restoreState.length | 0;
      },
    };

    const module = new WebAssembly.Module(new Uint8Array(wasmBytes));
    instance = new WebAssembly.Instance(module, { [CORE]: core });
    requireCondition(instance.exports.game_abi_version() === 1, "game ABI mismatch");
    runLifecycle(state, "init", function () {
      return instance.exports.game_init();
    });

    // A replay snapshot owns both guest bytes and the host RNG. init belongs
    // to construction, so discard any RNG it consumed before restoring.
    state.rng = configuration.rng >>> 0;
    state.stateLoaded = false;
    runLifecycle(state, "resume", function () {
      return instance.exports.game_resume();
    });
    requireCondition(state.stateLoaded, "game did not load state");

    const frames = [];
    let previousClock = null;
    for (const step of configuration.steps) {
      const buttons = step.buttons >>> 0;
      const clockMs = step.clockMs >>> 0;
      requireCondition((buttons & ~0x1ff) === 0, "unknown input bit");
      requireCondition(previousClock === null || clockMs >= previousClock, "clock moved backwards");
      state.buttons = buttons;
      state.clockMs = clockMs;
      runLifecycle(state, "tick", function () {
        return instance.exports.game_tick();
      });
      frames.push({ render: state.render, audio: state.audio });
      previousClock = clockMs;
    }
    return { frames: frames };
  }

  global.runTinyArcadeWebKitOracle = runTinyArcadeWebKitOracle;
})(globalThis);
