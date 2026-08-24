/**
 * Byte-for-byte parity tests: the TS emitter must reproduce the Rust
 * infra-manifest golden fixture exactly — same JSON text, same
 * content_hash. Regenerate the fixture via the command in README.md.
 */

'use strict';

const { readFileSync } = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');
const assert = require('node:assert/strict');

const { ManifestBuilder, ManifestError, SCHEMA_VERSION } = require('../src/manifest');

const FIXTURE = readFileSync(
  path.join(__dirname, 'fixtures', 'sample.lock.json'),
  'utf8',
).replace(/\r\n/g, '\n');

function buildSample() {
  return new ManifestBuilder('demo')
    .language('ts')
    .bucket('media', { public: false })
    .bucket('public-assets', { public: true })
    .topic('events')
    .topic('eu-events', { region: 'eu-central' })
    .database('main')
    .cron('nightly-rollup', '0 3 * * *')
    .secret('stripe-key')
    .service('api', {
      image: 'registry.example/demo-api:1.4.2',
      ports: { 8080: 8080, 9090: 9090 },
      env: { UPSTREAM: 'https://api.example.internal', RUST_LOG: 'info' },
      buckets: ['media', 'public-assets'],
      topics: ['events', 'eu-events'],
      databases: ['main'],
    })
    .service('worker', {
      image: 'registry.example/demo-worker:1.4.2',
      topics: ['events'],
    });
}

test('ts emitter reproduces rust fixture byte-for-byte', () => {
  const lock = buildSample().finalize();
  assert.equal(lock.toLock(), FIXTURE.replace(/\n$/, ''));
});

test('content_hash matches the rust-stamped hash', () => {
  const rustHash = JSON.parse(FIXTURE).content_hash;
  const lock = buildSample().finalize();
  assert.equal(lock.contentHash, rustHash);
});

test('verify catches post-finalize mutation', () => {
  const lock = buildSample().finalize();
  lock.verify(); // ok
  lock.manifest.services.api.image = 'evil:latest';
  assert.throws(() => lock.verify(), /content_hash mismatch/);
});

test('undeclared resource reference rejected', () => {
  assert.throws(
    () =>
      new ManifestBuilder('demo')
        .service('api', { image: 'nginx', buckets: ['ghost'] }),
    /undeclared/,
  );
});

test('bad names rejected', () => {
  for (const bad of ['UPPER', '9start', 'under_score', '-lead', 'x'.repeat(64), '']) {
    assert.throws(() => new ManifestBuilder(bad), /DNS label/, `app \`${bad}\``);
    assert.throws(
      () => new ManifestBuilder('ok').bucket(bad),
      ManifestError,
      `resource \`${bad}\``,
    );
  }
});

test('duplicate resource rejected; cron needs five fields', () => {
  assert.throws(() => new ManifestBuilder('d').bucket('a').bucket('a'), /duplicate/);
  assert.throws(() => new ManifestBuilder('d').cron('c', '* * *'), /5-field/);
});

test('schema version constant matches rust crate', () => {
  assert.equal(SCHEMA_VERSION, 1);
});
