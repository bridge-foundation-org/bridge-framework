/**
 * ManifestBuilder — TypeScript emitter for bridgebase.lock.json.
 *
 * Produces the exact schema owned by Rust `infra-manifest` (ADR-0001):
 * DNS-label name rules, schema version gate, referential integrity, and a
 * SHA-256 content_hash over the canonical serialization. `finalize()`
 * stamps the hash; `verify()` re-checks it so post-finalize mutation is
 * always caught.
 */

'use strict';

const { pretty, compact, sha256Hex } = require('./canonical');

/** Canonical HASH form: COMPACT json — mirrors Rust finalize()/verify()
 * which use serde_json::to_vec (compact). Lock TEXT stays pretty; only the
 * hashed representation is compact. */
function hashInput(obj) {
  return compact(obj);
}

const SCHEMA_VERSION = 1;
const MIN_SUPPORTED = 1;

const NAME_RE = /^[a-z][a-z0-9-]{0,62}$/;

class ManifestError extends Error {
  constructor(msg) {
    super(msg);
    this.name = 'ManifestError';
  }
}

function validateName(kind, name) {
  if (typeof name !== 'string' || !NAME_RE.test(name)) {
    throw new ManifestError(
      `${kind} name \`${name}\` must be a lowercase DNS label (a-z, 0-9, '-', <=63 chars, starting with a letter)`,
    );
  }
}

/** Build the plain-object manifest in Rust field order. */
function toObject(m) {
  const services = {};
  for (const name of Object.keys(m.services).sort()) {
    // Serialize via the same elision-aware path used at declaration time,
    // so post-hoc mutations of the internal record still emit correctly.
    services[name] = buildServiceObject(m.services[name]);
  }
  const resources = {};
  for (const name of Object.keys(m.resources).sort()) {
    resources[name] = m.resources[name]; // already internally-tagged objects
  }
  const out = { schema_version: m.schema_version, app: m.app };
  if (m.language != null) out.language = m.language;
  out.services = services;
  out.resources = resources;
  if (m.content_hash != null) out.content_hash = m.content_hash;
  return out;
}

function sortMap(map) {
  const out = {};
  for (const k of Object.keys(map ?? {}).sort()) out[k] = map[k];
  return out;
}

function sortPorts(ports) {
  const out = {};
  for (const k of Object.keys(ports ?? {}).map(Number).sort((a, b) => a - b)) {
    out[k] = ports[k];
  }
  return out;
}

/** Build the service object with serde's elision rules: empty maps/vecs
 * are omitted entirely (Rust skip_serializing_if), present fields keep
 * declaration order. */
function buildServiceObject(spec) {
  const out = { image: spec.image };
  const ports = sortPorts(spec.ports);
  if (Object.keys(ports).length) out.ports = ports;
  const env = sortMap(spec.env);
  if (Object.keys(env).length) out.env = env;
  if (spec.buckets?.length) out.buckets = [...spec.buckets];
  if (spec.topics?.length) out.topics = [...spec.topics];
  if (spec.databases?.length) out.databases = [...spec.databases];
  return out;
}

class ManifestBuilder {
  constructor(app) {
    validateName('app', app);
    this.manifest = {
      schema_version: SCHEMA_VERSION,
      app,
      language: undefined,
      services: {},
      resources: {},
      content_hash: undefined,
    };
    this.finalized = false;
  }

  language(lang) {
    this.assertOpen();
    this.manifest.language = lang;
    return this;
  }

  bucket(name, opts = {}) {
    this.assertOpen();
    validateName('resource', name);
    this.dedupe(name);
    this.manifest.resources[name] = { type: 'bucket', public: !!opts.public };
    return this;
  }

  topic(name, opts = {}) {
    this.assertOpen();
    validateName('resource', name);
    this.dedupe(name);
    const r = { type: 'topic' };
    if (opts.region != null) r.region = opts.region;
    this.manifest.resources[name] = r;
    return this;
  }

  database(name, engine = 'postgres') {
    this.assertOpen();
    validateName('resource', name);
    this.dedupe(name);
    if (engine !== 'postgres') throw new ManifestError(`unsupported db engine \`${engine}\``);
    this.manifest.resources[name] = { type: 'database', engine };
    return this;
  }

  cron(name, schedule) {
    this.assertOpen();
    validateName('resource', name);
    this.dedupe(name);
    if (schedule.split(/\s+/).length !== 5) {
      throw new ManifestError(`cron \`${name}\` schedule must be a 5-field cron expression`);
    }
    this.manifest.resources[name] = { type: 'cron', schedule };
    return this;
  }

  secret(name) {
    this.assertOpen();
    validateName('resource', name);
    this.dedupe(name);
    this.manifest.resources[name] = { type: 'secret' };
    return this;
  }

  service(name, spec) {
    this.assertOpen();
    validateName('service', name);
    if (!spec || typeof spec.image !== 'string' || !spec.image.trim()) {
      throw new ManifestError(`service \`${name}\` requires a non-empty image`);
    }
    for (const ref of [
      ...(spec.buckets ?? []),
      ...(spec.topics ?? []),
      ...(spec.databases ?? []),
    ]) {
      if (!(ref in this.manifest.resources)) {
        throw new ManifestError(
          `service \`${name}\` references undeclared resource \`${ref}\`; declare it before the service`,
        );
      }
    }
    // Note: unlike the CLI-facing builder flow above (declare-before-use),
    // resources declared AFTER addService are validated at finalize().
    this.manifest.services[name] = buildServiceObject(spec);
    return this;
  }

  dedupe(name) {
    if (name in this.manifest.resources) {
      throw new ManifestError(`duplicate resource name \`${name}\``);
    }
  }

  assertOpen() {
    if (this.finalized) {
      throw new ManifestError('manifest already finalized; start a new builder');
    }
  }

  /** Validate + stamp content_hash. Returns a frozen manifest handle. */
  finalize() {
    this.validateRefs();
    this.finalized = true;
    const obj = toObject(this.manifest);
    const canonical = hashInput({ ...obj, content_hash: undefined });
    this.manifest.content_hash = sha256Hex(canonical);
    return new FinalizedManifest(this.manifest);
  }

  validateRefs() {
    for (const [name, svc] of Object.entries(this.manifest.services)) {
      for (const kind of ['buckets', 'topics', 'databases']) {
        for (const ref of svc[kind] ?? []) {
          if (!(ref in this.manifest.resources)) {
            throw new ManifestError(
              `service \`${name}\` references undeclared ${kind.slice(0, -1)} \`${ref}\``,
            );
          }
        }
      }
    }
  }
}

class FinalizedManifest {
  constructor(manifest) {
    this.manifest = manifest;
  }

  /** Canonical lock-file text (pretty JSON, sorted keys). */
  toLock() {
    return pretty(toObject(this.manifest));
  }

  toObject() {
    return toObject(this.manifest);
  }

  get contentHash() {
    return this.manifest.content_hash;
  }

  /** Throws unless contents still hash to the stamped value. */
  verify() {
    const obj = toObject(this.manifest);
    const canonical = hashInput({ ...obj, content_hash: undefined });
    const actual = sha256Hex(canonical);
    if (actual !== this.manifest.content_hash) {
      throw new ManifestError(
        `content_hash mismatch: lock says ${this.manifest.content_hash}, contents hash to ${actual}; regenerate the lock`,
      );
    }
  }
}

module.exports = {
  ManifestBuilder,
  FinalizedManifest,
  ManifestError,
  SCHEMA_VERSION,
  MIN_SUPPORTED,
};
