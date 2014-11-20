Libaio Binding
==============

This crate implements a binding for Linux's libaio, for async block IO.

It presents several related APIs:
 * `raw`, which is a fairly direct mapping of the AIO syscalls to Rust
 * `chan`, a channel-oriented interface for submitting AIO operations and getting their results,
 * `future`, a function-oriented interface which returns futures for results

There is also a set of utility modules:
 * `buf`, which defines RdBuf and WrBuf traits, and some implementations for slices and Vec
 * `directio`, for opening direct IO files (preferred for async IO)
 * `aligned`, for allocating suitably aligned memory for direct IO.

This is still very much a work in progress, and the API is not at all stable yet.

Jeremy Fitzhardinge <jeremy@goop.org>
