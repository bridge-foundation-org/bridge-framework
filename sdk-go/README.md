# @bridgebase/sdk-go

Go SDK for BridgeBase. Emits `bridgebase.lock.json` **byte-identically** to
the Rust `infra-manifest` crate and the TypeScript SDK — the golden-fixture
test pins the exact bytes, content hash included.

## Usage

```go
package main

import (
	"fmt"
	bb "github.com/bridgebase/sdk-go"
)

func main() {
	b, _ := bb.NewBuilder("shop")
	b.Language("go")
	b.Bucket("media", bb.BucketOpts{})
	b.Database("main", "postgres")
	b.Service("api", bb.ServiceSpec{
		Image:     "registry.example/shop-api:1.0.0",
		Ports:     map[int]int{8080: 8080},
		Buckets:   []string{"media"},
		Databases: []string{"main"},
	})
	lock, err := b.Finalize()
	if err != nil {
		panic(err)
	}
	fmt.Println(lock.ToLock()) // canonical lock text; write to bridgebase.lock.json
}
```

## Guarantees

- `ToLock()` output is byte-for-byte what the CLI's schema crate produces
  for the same manifest (declaration-order struct fields, sorted map keys,
  numeric port ordering, serde-style elision of empty collections).
- `ContentHash()` equals the SHA-256 stamped by Rust/TS emitters.
- `Verify()` catches any post-finalize mutation.

Errors from chained builder calls do not break the chain; the first error
is returned by `Finalize()`.

## Development

```
go test ./...   # includes TestGoldenFixtureByteParity against testdata/
gofmt -l .      # must print nothing
go vet ./...
```
