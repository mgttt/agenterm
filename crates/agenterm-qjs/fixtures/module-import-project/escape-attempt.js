// Fixture for tests/module_imports.rs — must always be rejected by
// ProjectModuleResolver's root confinement (see module_resolver.rs),
// regardless of whether the target path exists.
import { x } from '../../../../../../etc/passwd.js';

export function entry() {
  return x;
}
