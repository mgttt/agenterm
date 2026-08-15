# Read the current thread name with `pthread_getname_np`

macOS example. Obtain the current `pthread_t` from native code with
`pthread_self()`, bind its integer representation as `thread`, and bind a
caller-owned NUL-initialized character buffer as `name`.

```lisp
(dlcall "libSystem.B.dylib" "pthread_getname_np" "i32" "u64" thread "ptr" name "u64" 64)
```

The return value is a status code. Only when it is zero may the caller read
`name` as a C string; an empty string is a valid thread name. The caller owns
both the thread handle representation and the output buffer.
