/**
 * Canonical serialization for bridgebase.lock.json.
 *
 * Must match Rust `infra-manifest` byte-for-byte. serde_json rules:
 * - STRUCT fields serialize in DECLARATION order (not sorted):
 *     manifest: schema_version, app, [language], services, resources, [content_hash]
 *     service:  image, ports, env, buckets, topics, databases
 *     resource: internally-tagged {type, ...variant fields in decl order}
 *   e.g. bucket {type,public}, topic {type,[region]}, database {type,engine},
 *   cron {type,schedule}, secret {type}
 * - MAPS (services/resources/ports/env) serialize KEY-SORTED (BTreeMap)
 * - Option::None / empty map / empty vec are ELIDED (skip_serializing_if)
 * - lock text is pretty (2-space); the HASH input is compact (to_vec)
 */

'use strict';

const { createHash } = require('node:crypto');

/** Drop undefined/null values WITHOUT reordering (struct field order is
 * meaningful). Empty arrays/objects stay elided by the builders themselves;
 * this only cleans optional fields. */
function prune(value) {
  if (Array.isArray(value)) return value;
  if (value && typeof value === 'object') {
    const out = {};
    for (const k of Object.keys(value)) {
      if (value[k] !== undefined && value[k] !== null) out[k] = value[k];
    }
    return out;
  }
  return value;
}

/** Pretty lock text (serde_json::to_string_pretty equivalent). */
function pretty(manifestObj) {
  return JSON.stringify(prune(manifestObj), null, 2);
}

/** Compact hash input (serde_json::to_vec equivalent → UTF-8 text). */
function compact(manifestObj) {
  return JSON.stringify(prune(manifestObj));
}

function sha256Hex(text) {
  return createHash('sha256').update(text, 'utf8').digest('hex');
}

module.exports = { pretty, compact, sha256Hex };
