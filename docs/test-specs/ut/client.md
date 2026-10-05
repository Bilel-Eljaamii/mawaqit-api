# Test Spec: `ut/client.rs` — client helpers and validation, public API

- **Tier:** unit (`cargo test --test ut`), offline, deterministic, no I/O.
- **Target:** `minutes_between`, `is_valid_slug`, and the SOCKS5/Tor proxy
  builders (moved out of `src/client.rs`; proxy validation now asserted
  through the public `with_socks_proxy` instead of the private validator).
- **Contract:** the pure helpers behave per their docs; every proxy address
  that is not exactly `socks5h://host[:port]` fails fast with
  `InvalidProxy`; builders compose in any order.

## Tests

### `minutes_between_handles_wrap`
b−a in minutes, with midnight wrap (23:30 → 00:10 = 40).

### `slug_length_is_bounded`
Review M1: 128 bytes of `a` is valid; 129 is not; a 100 KB blob is not.

### `proxy_validation_accepts_socks5h`
`socks5h://` addresses — bare host (port defaults to 9050), explicit
9050/9150, IPv6, user:pass — all build a client successfully.

### `proxy_validation_rejects_everything_that_is_not_socks5h`
Empty, whitespace, garbage, scheme-less, plain `socks5` (local DNS defeats
Tor), http/https/ftp, empty host, path/query/fragment — all fail with
`MawaqitError::InvalidProxy`.

### `builders_compose_in_any_order`
proxy → disk-cache and disk-cache → timeouts → proxy both construct.

### `invalid_proxy_fails_fast`
The builder returns the typed error immediately; nothing touches the
network.

## Run

```sh
cargo test --test ut client
```

A failure means hostile proxy addresses can redirect traffic (DNS leak) or
helpers misbehave on their documented edges.

The private `resolve_timeouts` matrix (proxy raises defaults, explicit
wins) is intentionally not ported: it is documented behavior observable
only through the transport.
