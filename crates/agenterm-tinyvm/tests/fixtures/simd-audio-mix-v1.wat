(module
  (memory (export "memory") 1)

  ;; Mix eight signed 16-bit PCM samples with standard saturating arithmetic.
  ;; The function keeps v128 internal so an ordinary i32 host ABI can drive it.
  (func (export "mix") (param $left i32) (param $right i32) (param $output i32)
    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i16x8.add_sat_s
    v128.store)

  ;; Remove eight signed 16-bit PCM lanes with the matching saturation rules.
  (func (export "subtract") (param $left i32) (param $right i32) (param $output i32)
    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i16x8.sub_sat_s
    v128.store)
)
