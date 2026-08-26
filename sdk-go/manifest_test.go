package bridgebase

import (
	"os"
	"strings"
	"testing"
)

// buildSample mirrors the TS SDK test's buildSample() exactly — same app,
// same declaration order — so the golden fixture pins cross-SDK parity.
func buildSample(t *testing.T) *FinalizedManifest {
	t.Helper()
	b, err := NewBuilder("demo")
	if err != nil {
		t.Fatalf("NewBuilder: %v", err)
	}
	b.Language("ts")
	b.Bucket("media", BucketOpts{Public: false})
	b.Bucket("public-assets", BucketOpts{Public: true})
	b.Topic("events", TopicOpts{})
	eu := "eu-central"
	b.Topic("eu-events", TopicOpts{Region: &eu})
	b.Database("main", "postgres")
	b.Cron("nightly-rollup", "0 3 * * *")
	b.Secret("stripe-key")
	b.Service("api", ServiceSpec{
		Image:     "registry.example/demo-api:1.4.2",
		Ports:     map[int]int{8080: 8080, 9090: 9090},
		Env:       map[string]string{"UPSTREAM": "https://api.example.internal", "RUST_LOG": "info"},
		Buckets:   []string{"media", "public-assets"},
		Topics:    []string{"events", "eu-events"},
		Databases: []string{"main"},
	})
	b.Service("worker", ServiceSpec{
		Image:  "registry.example/demo-worker:1.4.2",
		Topics: []string{"events"},
	})
	fm, err := b.Finalize()
	if err != nil {
		t.Fatalf("Finalize: %v", err)
	}
	return fm
}

// TestGoldenFixtureByteParity is THE parity contract: the Go emitter must
// reproduce sdk-rust/sdk-ts fixtures byte-for-byte, hash included.
func TestGoldenFixtureByteParity(t *testing.T) {
	want, err := os.ReadFile("testdata/sample.lock.json")
	if err != nil {
		t.Fatalf("fixture: %v", err)
	}
	wantText := strings.ReplaceAll(string(want), "\r\n", "\n")

	fm := buildSample(t)
	got := fm.ToLock()

	if got != wantText {
		t.Fatalf("lock text mismatch\n--- got ---\n%s\n--- want ---\n%s", got, wantText)
	}
	if fm.ContentHash() != "4ab6e326f1df50c77348c0681e274e6682604dfc719aacdc36aaaaf7c50c13c7" {
		t.Fatalf("content_hash mismatch: %s", fm.ContentHash())
	}
}

func TestVerifyDetectsTampering(t *testing.T) {
	fm := buildSample(t)
	if err := fm.Verify(); err != nil {
		t.Fatalf("pristine manifest must verify: %v", err)
	}
	fm.obj.app = "tampered"
	if err := fm.Verify(); err == nil {
		t.Fatal("mutated manifest must fail Verify")
	}
}

func TestBuilderErrorsSurfaceAtFinalize(t *testing.T) {
	b, _ := NewBuilder("demo")
	b.Database("main", "mysql") // unsupported engine
	if _, err := b.Finalize(); err == nil {
		t.Fatal("Finalize must return the stored builder error")
	} else if !strings.Contains(err.Error(), "unsupported db engine") {
		t.Fatalf("wrong error: %v", err)
	}
}

func TestUndeclaredResourceRefRejected(t *testing.T) {
	b, _ := NewBuilder("demo")
	b.Service("api", ServiceSpec{Image: "nginx", Databases: []string{"nope"}})
	_, err := b.Finalize()
	if err == nil || !strings.Contains(err.Error(), "undeclared resource") {
		t.Fatalf("want undeclared-resource error, got %v", err)
	}
}

func TestDuplicateAndBadNamesRejected(t *testing.T) {
	b, _ := NewBuilder("demo")
	b.Bucket("x", BucketOpts{})
	b.Bucket("x", BucketOpts{})
	if _, err := b.Finalize(); err == nil || !strings.Contains(err.Error(), "duplicate resource") {
		t.Fatalf("want duplicate error, got %v", err)
	}

	if _, err := NewBuilder("Bad_Name"); err == nil {
		t.Fatal("bad app name must be rejected at construction")
	}

	b2, _ := NewBuilder("demo")
	b2.Bucket("X", BucketOpts{})
	if _, err := b2.Finalize(); err == nil || !strings.Contains(err.Error(), "DNS label") {
		t.Fatalf("want DNS-label error, got %v", err)
	}
}

func TestCronRequiresFiveFields(t *testing.T) {
	b, _ := NewBuilder("demo")
	b.Cron("job", "* * * *") // 4 fields
	if _, err := b.Finalize(); err == nil || !strings.Contains(err.Error(), "5-field cron") {
		t.Fatalf("want cron error, got %v", err)
	}
}

func TestEmptyCollectionsElided(t *testing.T) {
	b, _ := NewBuilder("minimal")
	b.Service("only", ServiceSpec{Image: "nginx:alpine"})
	fm, err := b.Finalize()
	if err != nil {
		t.Fatalf("Finalize: %v", err)
	}
	lock := fm.ToLock()
	for _, banned := range []string{"ports", `"env"`, "buckets", "topics", "databases"} {
		if strings.Contains(lock, banned) {
			t.Fatalf("empty field %q must be elided:\n%s", banned, lock)
		}
	}
	if !strings.Contains(lock, `"language": "go"`) == false && b.language != nil {
		t.Fatal("unexpected language emission")
	}
}
