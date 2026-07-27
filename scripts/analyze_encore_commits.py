#!/usr/bin/env python3
"""
Analyze Encore commits and cross-check with Bridge daemon implementation.
Identifies gaps and missing features.
"""

import json
import re
from collections import defaultdict
from pathlib import Path

# Load commits
with open('e-commits/commits.json', 'r', encoding='utf-8') as f:
    commits = json.load(f)

# Feature categories from commit subjects
FEATURE_KEYWORDS = {
    'Metrics': r'(metrics|monitoring|prometheus|exporters)',
    'Tracing': r'(tracing|spans|traces|opentelemetry)',
    'Authentication': r'(auth|jwt|oauth|credentials|login)',
    'Rate Limiting': r'(rate.?limit|ratelimit|throttle)',
    'Caching': r'(cache|redis|miniredis)',
    'Pub/Sub': r'(pubsub|publish|subscribe|messaging|topics|nsq|sns|sqs)',
    'Database': r'(database|sqldb|postgres|migration|sql)',
    'HTTP': r'(http|rest|api|endpoint|request|response)',
    'Middleware': r'(middleware|handler|interceptor)',
    'Configuration': r'(config|configuration|settings|toml)',
    'CLI': r'(cli|command|daemon|cmd)',
    'Code Generation': r'(codegen|client|typescript|go|openapi)',
    'Docker': r'(docker|container|compose)',
    'Secrets': r'(secret|vault|credentials)',
    'Cron': r'(cron|schedule|job|timing)',
    'Streaming': r'(stream|sse|websocket|ws)',
    'Testing': r'(test|e2e|integration)',
    'Logging': r'(log|logger|log.level)',
    'Error Handling': r'(error|exception|err)',
    'Security': r'(security|audit|vulnerability)',
    'Performance': r'(performance|optimize|profil|bench)',
    'Documentation': r'(doc|readme|guide|tutorial)',
}

# Current daemon modules
DAEMON_MODULES = [
    'auth', 'autocomplete', 'config', 'config_schema', 'context', 'cron',
    'errors', 'go_codegen', 'http', 'logger', 'metrics', 'metrics_exporters',
    'middleware', 'pubsub', 'pubsub_provider', 'ratelimit', 'redis_cluster',
    'registry', 'scaffold', 'schema_introspect', 'secrets', 'services',
    'shutdown', 'sqldb', 'state', 'streaming', 'tcp', 'transactions',
    'transport', 'tracing', 'watcher', 'perf_profiler', 'security_audit'
]

# Categorize commits
categories = defaultdict(list)
uncategorized = []

for commit in commits:
    subject = commit.get('subject', '').lower()
    categorized = False
    
    for category, pattern in FEATURE_KEYWORDS.items():
        if re.search(pattern, subject):
            categories[category].append(commit)
            categorized = True
            break
    
    if not categorized:
        uncategorized.append(commit)

# Print analysis
print("=" * 80)
print("ENCORE COMMITS ANALYSIS VS BRIDGE DAEMON IMPLEMENTATION")
print("=" * 80)
print(f"\nTotal Commits: {len(commits)}")
print(f"Daemon Modules Implemented: {len(DAEMON_MODULES)}")
print("\n" + "=" * 80)
print("FEATURE CATEGORY BREAKDOWN")
print("=" * 80)

implementation_status = {
    'Metrics': 'metrics, metrics_exporters',
    'Tracing': 'tracing',
    'Authentication': 'auth',
    'Rate Limiting': 'ratelimit',
    'Caching': 'redis_cluster, (miniredis external)',
    'Pub/Sub': 'pubsub, pubsub_provider',
    'Database': 'sqldb, schema_introspect, transactions',
    'HTTP': 'http, transport',
    'Middleware': 'middleware',
    'Configuration': 'config, config_schema',
    'CLI': 'autocomplete, scaffold',
    'Code Generation': 'go_codegen',
    'Docker': '(external)',
    'Secrets': 'secrets',
    'Cron': 'cron',
    'Streaming': 'streaming',
    'Logging': 'logger',
    'Error Handling': 'errors',
    'Security': 'security_audit',
    'Performance': 'perf_profiler',
}

for category in sorted(categories.keys()):
    commits_in_cat = categories[category]
    impl_status = implementation_status.get(category, '?')
    print(f"\n{category.upper()}")
    print(f"  Commits: {len(commits_in_cat)}")
    print(f"  Daemon Modules: {impl_status}")
    print(f"  Status: {'IMPLEMENTED' if impl_status != '?' else 'PARTIAL'}")
    
    # Show key commits for this category
    key_commits = [c for c in commits_in_cat if c['index'] > 1990][:3]
    if key_commits:
        print(f"  Recent commits:")
        for c in key_commits:
            print(f"    - [{c['index']}] {c['subject'][:60]}")

print(f"\nUncategorized: {len(uncategorized)} commits")

print("\n" + "=" * 80)
print("FEATURE COMPLETENESS ASSESSMENT")
print("=" * 80)

implemented_count = sum(1 for cat in categories.keys() if implementation_status.get(cat, '?') != '?')
total_categories = len(categories)

print(f"\nCategories with implementation: {implemented_count}/{total_categories}")
print(f"Coverage: {100 * implemented_count / total_categories:.1f}%")

print("\n" + "=" * 80)
print("MISSING OR PARTIALLY IMPLEMENTED FEATURES")
print("=" * 80)

missing = []
for category in sorted(categories.keys()):
    impl = implementation_status.get(category, '')
    if '?' in impl or 'external' in impl or impl == '':
        missing.append((category, len(categories[category]), impl))

for cat, count, impl in sorted(missing, key=lambda x: -x[1])[:10]:
    print(f"\n{cat}: {count} commits")
    print(f"  Current: {impl if impl else 'NOT IMPLEMENTED'}")
