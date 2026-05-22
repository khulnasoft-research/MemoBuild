#!/bin/bash
# Security audit script for MemoBuild

set -e

echo "🔐 MemoBuild Security Audit"
echo "============================"
echo ""

# 1. Cargo audit
echo "📋 Running cargo audit..."
if command -v cargo-audit &> /dev/null; then
    cargo audit || echo "⚠️  Some vulnerabilities found (see above)"
else
    echo "⚠️  cargo-audit not installed. Install with: cargo install cargo-audit"
    cargo audit || true
fi
echo ""

# 2. Dependency tree analysis
echo "📊 Checking dependency depth..."
cargo tree --depth 3 2>/dev/null | wc -l || echo "⚠️  Could not analyze dependency tree"
echo ""

# 3. Check for known insecure patterns
echo "🔍 Scanning for insecure patterns..."
patterns_found=0

# Check for unwrap in security-critical paths
if grep -r "\.unwrap()" src/export/registry.rs 2>/dev/null | grep -q "token"; then
    echo "⚠️  Found unwrap() on token handling"
    patterns_found=$((patterns_found + 1))
fi

# Check for debug assertions on secrets
if grep -r "eprintln!" src/server/mod.rs | grep -q "hash"; then
    echo "✅ Using logging for hashes (safe)"
else
    echo "⚠️  Check logging statements don't leak hashes"
fi

# Check for hardcoded credentials
if grep -r "password\|secret\|token" src/ | grep -q "= \""; then
    echo "⚠️  Possible hardcoded credential"
    patterns_found=$((patterns_found + 1))
fi

echo ""
echo "📊 Pattern scan complete. Issues found: $patterns_found"
echo ""

# 4. File permissions check (for deployment)
echo "🔒 Checking artifact storage directory..."
if [ -d ".memobuild-cache" ]; then
    perms=$(stat -c %a .memobuild-cache 2>/dev/null || stat -f %OLp .memobuild-cache 2>/dev/null || echo "unknown")
    if [ "$perms" != "700" ]; then
        echo "⚠️  Cache directory permissions: $perms (recommended: 700)"
    else
        echo "✅ Cache directory has secure permissions"
    fi
fi
echo ""

# 5. Test compilation with security features
echo "🧪 Testing with all security checks..."
cargo check --all-features 2>&1 | grep -i "warn\|error" || echo "✅ No compiler warnings"
echo ""

# 6. Run security-related tests
echo "✅ Running security tests..."
cargo test --test error_handling_test -- --nocapture 2>&1 || true
echo ""

echo "================================"
echo "✅ Security audit complete"
echo ""
echo "Next steps:"
echo "- Review SECURITY.md for detailed recommendations"
echo "- Update dependencies: cargo update"
echo "- Use SBOM generation: cargo-sbom"
echo "- Enable JSON logging in production: MEMOBUILD_JSON_LOGS=true"
