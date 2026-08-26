// Package bridgebase is the Go SDK for BridgeBase (goal §3: sdk-go).
//
// It emits `bridgebase.lock.json` byte-identically to the canonical Rust
// `infra-manifest` crate (ADR-0001) and the TypeScript SDK — the golden
// fixture test pins this: same JSON text, same SHA-256 content_hash.
//
// Canonical serialization rules (mirror serde_json exactly):
//   - STRUCT fields in DECLARATION order:
//     manifest: schema_version, app, [language], services, resources,
//     [content_hash]
//     service:  image, ports, env, buckets, topics, databases
//     resource: {type, ...variant fields}: bucket{public},
//     topic{[region]}, database{engine}, cron{schedule}, secret{}
//   - MAPS (services/resources/ports/env) KEY-SORTED (BTreeMap semantics);
//     port keys are numeric strings sorted numerically by the schema crate
//   - nil/empty map/empty slice ELIDED (skip_serializing_if)
//   - lock text is pretty (2-space indent); the HASH input is compact
//
// encoding/json cannot be used for emission because it always sorts object
// keys alphabetically, destroying declaration order — so this package ships
// its own minimal canonical writer (canonical.go).
package bridgebase

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strconv"
	"strings"
)

const (
	// SchemaVersion is the manifest schema version this SDK writes.
	SchemaVersion = 1
	// MinSupportedSchema is the oldest schema this SDK accepts.
	MinSupportedSchema = 1
)

// ManifestError mirrors ManifestError in the Rust/TS SDKs.
type ManifestError struct{ Msg string }

func (e *ManifestError) Error() string { return e.Msg }

func errf(format string, args ...any) error {
	return &ManifestError{Msg: fmt.Sprintf(format, args...)}
}

// validName: lowercase DNS label (a-z, 0-9, '-', ≤63 chars, letter first) —
// identical to validate_name in infra-manifest.
func validName(name string) bool {
	if name == "" || len(name) > 63 {
		return false
	}
	if name[0] < 'a' || name[0] > 'z' {
		return false
	}
	for i := 1; i < len(name); i++ {
		c := name[i]
		if (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-' {
			continue
		}
		return false
	}
	return true
}

func validateName(kind, name string) error {
	if !validName(name) {
		return errf("%s name `"+name+"` must be a lowercase DNS label (a-z, 0-9, '-', <=63 chars, starting with a letter)", kind)
	}
	return nil
}

// ── Public declaration types ─────────────────────────────────────────────────

// BucketOpts tunes Bucket.
type BucketOpts struct{ Public bool }

// TopicOpts tunes Topic; nil Region elides the field from output.
type TopicOpts struct{ Region *string }

// StringPtr is a small helper for TopicOpts{Region: StringPtr("eu-central")}.
func StringPtr(s string) *string { return &s }

// ServiceSpec declares one service. Empty maps/slices are ELIDED on emit,
// matching skip_serializing_if in the schema crate.
type ServiceSpec struct {
	Image     string
	Ports     map[int]int // host -> container
	Env       map[string]string
	Buckets   []string
	Topics    []string
	Databases []string
}

type resource struct {
	kind     string
	public   *bool   // bucket
	region   *string // topic
	engine   string  // database
	schedule string  // cron
}

// Builder constructs a manifest. Chain methods, then call Finalize. Method
// errors do not break the chain — the FIRST error is remembered and
// returned by Finalize (Go's answer to the TS SDK's throw-on-misuse).
type Builder struct {
	app       string
	language  *string
	services  map[string]ServiceSpec
	resources map[string]resource
	finalized bool
	fail      error
}

// NewBuilder starts a manifest for app (lowercase DNS label).
func NewBuilder(app string) (*Builder, error) {
	if err := validateName("app", app); err != nil {
		return nil, err
	}
	return &Builder{
		app:       app,
		services:  map[string]ServiceSpec{},
		resources: map[string]resource{},
	}, nil
}

func (b *Builder) open() bool {
	if b.fail == nil && b.finalized {
		b.fail = errf("manifest already finalized; start a new builder")
	}
	return b.fail == nil
}

// Language tags the app's primary language ("ts" | "rust" | "go").
func (b *Builder) Language(lang string) *Builder {
	if b.open() {
		b.language = &lang
	}
	return b
}

// Bucket declares S3-shaped object storage.
func (b *Builder) Bucket(name string, opts BucketOpts) *Builder {
	pub := opts.Public
	return b.putResource(name, resource{kind: "bucket", public: &pub})
}

// Topic declares pub/sub.
func (b *Builder) Topic(name string, opts TopicOpts) *Builder {
	return b.putResource(name, resource{kind: "topic", region: opts.Region})
}

// Database declares a managed database; engine must be "postgres".
func (b *Builder) Database(name, engine string) *Builder {
	if !b.open() {
		return b
	}
	if engine != "postgres" {
		b.fail = errf("unsupported db engine `" + engine + "`")
		return b
	}
	return b.putResource(name, resource{kind: "database", engine: engine})
}

// Cron declares a scheduled job; schedule must be a 5-field cron expression.
func (b *Builder) Cron(name, schedule string) *Builder {
	if !b.open() {
		return b
	}
	if len(strings.Fields(schedule)) != 5 {
		b.fail = errf("cron `" + name + "` schedule must be a 5-field cron expression")
		return b
	}
	return b.putResource(name, resource{kind: "cron", schedule: schedule})
}

// Secret declares a secret reference.
func (b *Builder) Secret(name string) *Builder {
	return b.putResource(name, resource{kind: "secret"})
}

func (b *Builder) putResource(name string, r resource) *Builder {
	if !b.open() {
		return b
	}
	if err := validateName("resource", name); err != nil {
		b.fail = err
		return b
	}
	if _, dup := b.resources[name]; dup {
		b.fail = errf("duplicate resource name `" + name + "`")
		return b
	}
	b.resources[name] = r
	return b
}

// Service adds a service; every referenced resource must be declared first.
func (b *Builder) Service(name string, spec ServiceSpec) *Builder {
	if !b.open() {
		return b
	}
	if err := validateName("service", name); err != nil {
		b.fail = err
		return b
	}
	if strings.TrimSpace(spec.Image) == "" {
		b.fail = errf("service `" + name + "` requires a non-empty image")
		return b
	}
	for _, ref := range spec.Buckets {
		if _, ok := b.resources[ref]; !ok {
			b.fail = errf("service `" + name + "` references undeclared resource `" + ref + "`; declare it before the service")
			return b
		}
	}
	for _, ref := range spec.Topics {
		if _, ok := b.resources[ref]; !ok {
			b.fail = errf("service `" + name + "` references undeclared resource `" + ref + "`; declare it before the service")
			return b
		}
	}
	for _, ref := range spec.Databases {
		if _, ok := b.resources[ref]; !ok {
			b.fail = errf("service `" + name + "` references undeclared resource `" + ref + "`; declare it before the service")
			return b
		}
	}
	b.services[name] = spec
	return b
}

// Finalize validates everything and stamps content_hash. It also returns
// any error stored by earlier chained calls, so misuse surfaces exactly
// once, at the point the caller consumes the result.
func (b *Builder) Finalize() (*FinalizedManifest, error) {
	if b.fail != nil {
		return nil, b.fail
	}
	if b.finalized {
		return nil, errf("manifest already finalized; start a new builder")
	}
	if err := b.validateRefs(); err != nil {
		return nil, err
	}
	b.finalized = true

	obj := b.manifestObject()
	obj.contentHash = ""
	canonical := emitCompact(obj)
	sum := sha256.Sum256([]byte(canonical))
	hash := hex.EncodeToString(sum[:])
	obj.contentHash = hash
	return &FinalizedManifest{obj: obj, hash: hash}, nil
}

func (b *Builder) validateRefs() error {
	names := make([]string, 0, len(b.services))
	for n := range b.services {
		names = append(names, n)
	}
	sort.Strings(names)
	for _, name := range names {
		svc := b.services[name]
		for kind, refs := range map[string][]string{
			"bucket":   svc.Buckets,
			"topic":    svc.Topics,
			"database": svc.Databases,
		} {
			for _, ref := range refs {
				if _, ok := b.resources[ref]; !ok {
					return errf("service `" + name + "` references undeclared " + kind + " `" + ref + "`")
				}
			}
		}
	}
	return nil
}

// manifestObject assembles the ordered document in exact declaration order.
func (b *Builder) manifestObject() *manifestDoc {
	doc := &manifestDoc{
		schemaVersion: SchemaVersion,
		app:           b.app,
		language:      b.language,
		services:      map[string]*serviceDoc{},
		resources:     map[string]*resourceDoc{},
	}
	for name, svc := range b.services {
		doc.services[name] = serviceToObject(svc)
	}
	for name, r := range b.resources {
		doc.resources[name] = resourceToObject(r)
	}
	return doc
}

func serviceToObject(s ServiceSpec) *serviceDoc {
	d := &serviceDoc{
		image:     s.Image,
		ports:     s.Ports,
		env:       s.Env,
		buckets:   s.Buckets,
		topics:    s.Topics,
		databases: s.Databases,
	}
	return d
}

func resourceToObject(r resource) *resourceDoc {
	return &resourceDoc{
		kind:     r.kind,
		public:   r.public,
		region:   r.region,
		engine:   r.engine,
		schedule: r.schedule,
	}
}

// FinalizedManifest is the immutable result of Builder.Finalize.
type FinalizedManifest struct {
	obj  *manifestDoc
	hash string
}

// ToLock renders the canonical pretty lock text (2-space indent, LF lines,
// no trailing spaces) — byte-identical to serde_json::to_string_pretty.
func (f *FinalizedManifest) ToLock() string { return emitPretty(f.obj) }

// ContentHash returns the stamped SHA-256 hex digest.
func (f *FinalizedManifest) ContentHash() string { return f.hash }

// Verify re-checks that the contents still hash to the stamped value;
// any post-finalize mutation of exported copies is caught here.
func (f *FinalizedManifest) Verify() error {
	saved := f.obj.contentHash
	f.obj.contentHash = ""
	canonical := emitCompact(f.obj)
	f.obj.contentHash = saved
	sum := sha256.Sum256([]byte(canonical))
	actual := hex.EncodeToString(sum[:])
	if actual != f.hash {
		return errf("content_hash mismatch: lock says " + f.hash + ", contents hash to " + actual + "; regenerate the lock")
	}
	return nil
}

// portKeysNumeric sorts port keys numerically (schema crate uses
// BTreeMap<u16, u16>, i.e. numeric order — "9090" after "8080").
func portKeysNumeric(ports map[int]int) []int {
	keys := make([]int, 0, len(ports))
	for k := range ports {
		keys = append(keys, k)
	}
	sort.Ints(keys)
	return keys
}

// atoiKey renders a port key the way the schema crate does: bare integer.
func atoiKey(port int) string { return strconv.Itoa(port) }
