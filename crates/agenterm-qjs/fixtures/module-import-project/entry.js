// Fixture for tests/module_imports.rs — real multi-file ES module project,
// used to prove QJS-M5a..c work end to end against files that actually
// ship with the repo, not just tempdir strings inside unit tests.
import { value } from './lib/value.js';

export function entry() {
  print('imported value: ' + value);
  return value;
}
