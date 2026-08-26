// Canonical JSON emission — byte-identical to serde_json (see package doc).
//
// encoding/json is unusable here: it sorts keys alphabetically and HTML-
// escapes by default, while the schema crate emits struct fields in
// declaration order and escapes per RFC 8259 minimal-JSON rules. This file
// implements exactly the subset needed for bridgebase.lock.json.
package bridgebase

import (
	"sort"
	"strconv"
	"strings"
)

// manifestDoc mirrors infra_manifest::Manifest field order:
// schema_version, app, [language], services, resources, [content_hash].
type manifestDoc struct {
	schemaVersion int
	app           string
	language      *string
	services      map[string]*serviceDoc
	resources     map[string]*resourceDoc
	contentHash   string
}

// serviceDoc mirrors infra_manifest::Service order:
// image, ports, env, buckets, topics, databases.
type serviceDoc struct {
	image     string
	ports     map[int]int
	env       map[string]string
	buckets   []string
	topics    []string
	databases []string
}

// resourceDoc mirrors the internally-tagged enum Resource:
// {type, ...variant fields in declaration order}.
type resourceDoc struct {
	kind     string
	public   *bool   // bucket: always present (default false)
	region   *string // topic: elided when nil
	engine   string  // database
	schedule string  // cron
}

// ── Writers ──────────────────────────────────────────────────────────────────

func emitPretty(d *manifestDoc) string {
	var b strings.Builder
	b.WriteString("{\n")
	b.WriteString(`  "schema_version": ` + strconv.Itoa(d.schemaVersion) + ",\n")
	b.WriteString("  \"app\": " + quote(d.app))
	if d.language != nil {
		b.WriteString(",\n  \"language\": " + quote(*d.language))
	}
	// services: BTreeMap → key-sorted; empty map ELIDED.
	if len(d.services) > 0 {
		b.WriteString(",\n  \"services\": ")
		writeSortedMapOf(&b, d.services, func(v *serviceDoc) string { return writeServicePretty(v) }, 2)
	}
	// resources: same treatment.
	if len(d.resources) > 0 {
		b.WriteString(",\n  \"resources\": ")
		writeSortedMapOf(&b, d.resources, func(v *resourceDoc) string { return writeResourcePretty(v) }, 2)
	}
	if d.contentHash != "" {
		b.WriteString(",\n  \"content_hash\": " + quote(d.contentHash))
	}
	b.WriteString("\n}\n")
	return b.String()
}

func emitCompact(d *manifestDoc) string {
	var b strings.Builder
	b.WriteByte('{')
	b.WriteString(`"schema_version":` + strconv.Itoa(d.schemaVersion) + ",")
	b.WriteString(`"app":` + quote(d.app))
	if d.language != nil {
		b.WriteString(`,"language":` + quote(*d.language))
	}
	if len(d.services) > 0 {
		b.WriteString(`,"services":`)
		writeSortedMapOfCompact(&b, d.services, writeServiceCompact)
	}
	if len(d.resources) > 0 {
		b.WriteString(`,"resources":`)
		writeSortedMapOfCompact(&b, d.resources, writeResourceCompact)
	}
	if d.contentHash != "" {
		b.WriteString(`,"content_hash":` + quote(d.contentHash))
	}
	b.WriteByte('}')
	return b.String()
}

func writeServicePretty(s *serviceDoc) string {
	var b strings.Builder
	b.WriteString("{\n")
	b.WriteString("      \"image\": " + quote(s.image))
	if len(s.ports) > 0 {
		b.WriteString(",\n      \"ports\": ")
		b.WriteString(portsPretty(s.ports))
	}
	if len(s.env) > 0 {
		b.WriteString(",\n      \"env\": ")
		b.WriteString(stringMapPretty(s.env, 6))
	}
	if len(s.buckets) > 0 {
		b.WriteString(",\n      \"buckets\": ")
		b.WriteString(stringSlicePretty(s.buckets, 6))
	}
	if len(s.topics) > 0 {
		b.WriteString(",\n      \"topics\": ")
		b.WriteString(stringSlicePretty(s.topics, 6))
	}
	if len(s.databases) > 0 {
		b.WriteString(",\n      \"databases\": ")
		b.WriteString(stringSlicePretty(s.databases, 6))
	}
	b.WriteString("\n    }")
	return b.String()
}

func writeServiceCompact(s *serviceDoc) string {
	var b strings.Builder
	b.WriteByte('{')
	b.WriteString(`"image":` + quote(s.image))
	if len(s.ports) > 0 {
		b.WriteString(`,"ports":` + portsCompact(s.ports))
	}
	if len(s.env) > 0 {
		b.WriteString(`,"env":` + stringMapCompact(s.env))
	}
	if len(s.buckets) > 0 {
		b.WriteString(`,"buckets":` + stringSliceCompact(s.buckets))
	}
	if len(s.topics) > 0 {
		b.WriteString(`,"topics":` + stringSliceCompact(s.topics))
	}
	if len(s.databases) > 0 {
		b.WriteString(`,"databases":` + stringSliceCompact(s.databases))
	}
	b.WriteByte('}')
	return b.String()
}

func writeResourcePretty(r *resourceDoc) string {
	var b strings.Builder
	b.WriteString("{\n")
	b.WriteString("      \"type\": " + quote(r.kind))
	switch r.kind {
	case "bucket":
		// public always serializes (no skip_serializing_if on it).
		b.WriteString(",\n      \"public\": " + strconv.FormatBool(*r.public))
	case "topic":
		if r.region != nil {
			b.WriteString(",\n      \"region\": " + quote(*r.region))
		}
	case "database":
		b.WriteString(",\n      \"engine\": " + quote(r.engine))
	case "cron":
		b.WriteString(",\n      \"schedule\": " + quote(r.schedule))
	case "secret":
		// no variant fields
	}
	b.WriteString("\n    }")
	return b.String()
}

func writeResourceCompact(r *resourceDoc) string {
	var b strings.Builder
	b.WriteByte('{')
	b.WriteString(`"type":` + quote(r.kind))
	switch r.kind {
	case "bucket":
		b.WriteString(`,"public":` + strconv.FormatBool(*r.public))
	case "topic":
		if r.region != nil {
			b.WriteString(`,"region":` + quote(*r.region))
		}
	case "database":
		b.WriteString(`,"engine":` + quote(r.engine))
	case "cron":
		b.WriteString(`,"schedule":` + quote(r.schedule))
	}
	b.WriteByte('}')
	return b.String()
}

// ── Map/slice helpers ────────────────────────────────────────────────────────

func sortedStringKeys[M ~map[string]V, V any](m M) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

// portsPretty renders {"8080": 8080} with NUMERICALLY sorted keys
// (BTreeMap<u16,u16> semantics), values as bare integers. Nested at depth
// 3 (service field): entries at 8 spaces, closing brace at 6.
func portsPretty(ports map[int]int) string {
	keys := portKeysNumeric(ports)
	var b strings.Builder
	b.WriteString("{\n")
	for i, k := range keys {
		pk := atoiKey(k)
		b.WriteString("        " + quote(pk) + ": " + strconv.Itoa(ports[k]))
		if i < len(keys)-1 {
			b.WriteByte(',')
		}
		b.WriteByte('\n')
	}
	b.WriteString("      }")
	return b.String()
}

func portsCompact(ports map[int]int) string {
	keys := portKeysNumeric(ports)
	var b strings.Builder
	b.WriteByte('{')
	for i, k := range keys {
		pk := atoiKey(k)
		b.WriteString(quote(pk) + ":" + strconv.Itoa(ports[k]))
		if i < len(keys)-1 {
			b.WriteByte(',')
		}
	}
	b.WriteByte('}')
	return b.String()
}

func stringMapPretty(m map[string]string, indent int) string {
	pad := strings.Repeat(" ", indent+2)
	endPad := strings.Repeat(" ", indent)
	keys := sortedStringKeys(m)
	var b strings.Builder
	b.WriteString("{\n")
	for i, k := range keys {
		b.WriteString(pad + quote(k) + ": " + quote(m[k]))
		if i < len(keys)-1 {
			b.WriteByte(',')
		}
		b.WriteByte('\n')
	}
	b.WriteString(endPad + "}")
	return b.String()
}

func stringMapCompact(m map[string]string) string {
	keys := sortedStringKeys(m)
	var b strings.Builder
	b.WriteByte('{')
	for i, k := range keys {
		b.WriteString(quote(k) + ":" + quote(m[k]))
		if i < len(keys)-1 {
			b.WriteByte(',')
		}
	}
	b.WriteByte('}')
	return b.String()
}

func stringSlicePretty(items []string, indent int) string {
	pad := strings.Repeat(" ", indent+2)
	endPad := strings.Repeat(" ", indent)
	var b strings.Builder
	b.WriteString("[\n")
	for i, s := range items {
		b.WriteString(pad + quote(s))
		if i < len(items)-1 {
			b.WriteByte(',')
		}
		b.WriteByte('\n')
	}
	b.WriteString(endPad + "]")
	return b.String()
}

func stringSliceCompact(items []string) string {
	var b strings.Builder
	b.WriteByte('[')
	for i, s := range items {
		b.WriteString(quote(s))
		if i < len(items)-1 {
			b.WriteByte(',')
		}
	}
	b.WriteByte(']')
	return b.String()
}

// writeSortedMapOf writes a pretty object of T values at the given depth.
// Indentation contract (serde_json::to_string_pretty): an object nested at
// depth d opens its entries with 2*(d+1) spaces and closes with 2*d spaces.
func writeSortedMapOf[M ~map[string]V, V any](b *strings.Builder, m M, writeVal func(V) string, depth int) {
	keys := sortedStringKeys(m)
	entryPad := strings.Repeat(" ", 2*depth)
	closePad := strings.Repeat(" ", 2*(depth-1))
	b.WriteString("{\n")
	for i, k := range keys {
		b.WriteString(entryPad + quote(k) + ": " + writeVal(m[k]))
		if i < len(keys)-1 {
			b.WriteByte(',')
		}
		b.WriteByte('\n')
	}
	b.WriteString(closePad + "}")
}

func writeSortedMapOfCompact[M ~map[string]V, V any](b *strings.Builder, m M, writeVal func(V) string) {
	keys := sortedStringKeys(m)
	b.WriteByte('{')
	for i, k := range keys {
		b.WriteString(quote(k) + ":" + writeVal(m[k]))
		if i < len(keys)-1 {
			b.WriteByte(',')
		}
	}
	b.WriteByte('}')
}

// ── String escaping ──────────────────────────────────────────────────────────

const hexDigits = "0123456789abcdef"

// quote escapes per serde_json's default (RFC 8259): escapes ", \, and
// control chars; passes UTF-8 through except U+2028/U+2029 which Go's
// encoder escapes but serde does NOT — so we do NOT escape them either.
func quote(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			b.WriteString(`\"`)
		case '\\':
			b.WriteString(`\\`)
		case '\n':
			b.WriteString(`\n`)
		case '\r':
			b.WriteString(`\r`)
		case '\t':
			b.WriteString(`\t`)
		default:
			if r < 0x20 {
				b.WriteString(`\u00`)
				b.WriteByte(hexDigits[(r>>4)&0xf])
				b.WriteByte(hexDigits[r&0xf])
			} else {
				// UTF-8 passes through unescaped, matching serde_json
				// (it does not escape non-ASCII or U+2028/U+2029).
				b.WriteRune(r)
			}
		}
	}
	b.WriteByte('"')
	return b.String()
}
