# Transport plugins

`vpn21-core` deliberately keeps the transport layer behind a narrow trait so
that tomorrow we can replace leaf with, for instance, xray-core or add
lyrebird as an additional PT.

## The trait

```rust
pub trait TransportProvider: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> TransportCapabilities;
    fn start(&self, cfg: TransportConfig, cancel: CancellationToken)
        -> BoxFuture<'static, Result<TransportHandle>>;
}
```

`TransportConfig` is a provider-agnostic struct built from the user's
profile (`outbounds`, stream settings, reality keys, …). Each provider is
free to translate it into its own native representation.

`TransportHandle` exposes the SOCKS endpoint (address + credentials) the
upstream side of the tunnel is supposed to connect to, plus an async
`stop()` method that must honour the cancellation token within 5 seconds.

## Registering a new provider

1. Add a crate under `rust/vpn21-<name>-provider`.
2. Implement `TransportProvider`.
3. Register it in `vpn21_core::transport::registry::default_registry()`.
4. Expose a new enum variant in the Dart side (`TransportKind`).

## Planned providers

| provider   | status   | notes                                  |
| ---------- | -------- | -------------------------------------- |
| `leaf`     | shipped  | VLESS + REALITY + gRPC + Trojan + etc. |
| `xray`     | planned  | Blocked on stable C FFI                |
| `lyrebird` | planned  | Chained before VLESS for obfs4         |
