# Installation

> How to add blkit to your Go project and pin it to a known version.

## Requirements

- **Go 1.22 or later** (`go version` to check).

## Add the module

From within your Go module, run:

```bash
go get github.com/friendly-business-machines/blkit
```

This adds blkit to your `go.mod` and downloads the latest released version.

## Pinning a version

For reproducible builds, pin blkit to a specific release tag:

```bash
go get github.com/friendly-business-machines/blkit@v1.2.3
```

Replace `v1.2.3` with the release you want. You can also pin to a branch or a
commit SHA, though released tags are recommended for production use.

## Importing

The core package is imported as `bl`:

```go
import bl "github.com/friendly-business-machines/blkit/core"
```

This single import provides the whole logic layer — value types, the expression
engine, decision models, process classes, and data contracts. The optional
infrastructure packages (for example `blkit/messagegateway` and
`blkit/restserver`) are imported under their own paths as they become available.

## Keeping current

The Getting Started and Installation pages are reviewed with every release. If
you upgrade across a major version, check the release notes for migration
guidance.
