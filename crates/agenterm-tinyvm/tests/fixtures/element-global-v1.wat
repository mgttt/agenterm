(module
  (import "host" "seed" (global $seed externref))
  (table $refs (export "refs") 2 externref)

  ;; Active and passive element expressions both read the same immutable
  ;; imported reference throughout this instance.
  (elem (table $refs) (i32.const 0) externref (global.get 0))
  (elem $later externref (global.get 0))

  (func (export "install_passive")
    i32.const 1
    i32.const 0
    i32.const 1
    table.init $refs $later
    elem.drop $later))
