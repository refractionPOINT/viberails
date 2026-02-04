# Signing Code

This directory contains the code signing client and configuration for signing release binaries.

## Dependencies

Dependencies are pinned with hashes in `requirements.lock` for supply chain security.
The CI always uses this lockfile via `pip install --require-hashes`.

### Regenerating the Lockfile

When updating dependencies in `requirements.txt`, regenerate the lockfile:

```bash
cd signing

# Create virtual environment (pip-tools requires it)
python3 -m venv .venv
source .venv/bin/activate

# Install pip-tools
pip install pip-tools

# Generate lockfile with hashes
pip-compile --generate-hashes --output-file=requirements.lock requirements.txt

# Clean up
deactivate
rm -rf .venv
```

**Important**: Always commit both `requirements.txt` and `requirements.lock` together.

## Files

- `requirements.txt` - Direct dependencies (unpinned, for development)
- `requirements.lock` - Full dependency tree with pinned versions and hashes (for CI)
- `client.py` - Code signing client
- `config.json` - Signing configuration
- `lc_py/` - LimaCharlie Python utilities
- `entitlements/` - macOS entitlements for code signing
