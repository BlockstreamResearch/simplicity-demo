# SimplicityHL quickstart demo

This CLI showcases how to deploy and interact with Simplicity HL contracts.

It is a stripped-down version of [the CLI from the simplicity-contracts](https://github.com/BlockstreamResearch/simplicity-contracts) repository, including only the code needed for a Liquid Testnet transaction with the `p2pk.simf` contract.

It provides [basic](src/commands/basic.rs) CLI.

Use `--help` to understand what this command can do.

Refer to the [contracts README](../contracts/README.md) for more information about the contracts.

## How to Use

```bash
cargo run -p cli -- basic --help
```

See [example-run.md](assets/example-run.md) for actual usage of this CLI.

## License

Dual-licensed under either of:
- Apache License, Version 2.0 (Apache-2.0)
- MIT license (MIT)

at your option.
