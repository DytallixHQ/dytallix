# Algorithm Guard Example

Reference contract showing how to gate submissions on the algorithm registry.

It demonstrates:

- algorithm allowlisting via the shared registry module
- capability-based checks before accepting work
- emergency circuit-breaker inheritance

Run:

```bash
cargo test --manifest-path examples/algorithm_guard/Cargo.toml
```
