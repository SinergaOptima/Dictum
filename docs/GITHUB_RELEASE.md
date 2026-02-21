# GitHub + Release Setup

## 1) Connect local repo to GitHub

Check remote:

```powershell
git remote -v
```

If needed, set remote:

```powershell
git remote set-url origin https://github.com/<your-org-or-user>/Dictum.git
```

Authenticate (recommended with GitHub CLI):

```powershell
gh auth login
```

Push branch:

```powershell
git push -u origin main
```

## 2) Configure release workflow secrets

Repository Settings -> Secrets and variables -> Actions -> New repository secret

Add:
- `WINDOWS_CERT_BASE64`: Base64-encoded `.pfx` signing certificate
- `WINDOWS_CERT_PASSWORD`: Password for the `.pfx`

If these are not present, release artifacts are built but not Authenticode-signed.

## 3) Trigger release

Option A: Push a version tag (auto release upload):

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Option B: Run manually:
- GitHub -> Actions -> `Windows Release` -> `Run workflow`

## 4) Produced artifacts

The workflow generates and uploads:
- `dictum.exe`
- `Dictum_<version>_x64-setup.exe`
- `SHA256SUMS.txt`

On tag builds, these are attached to the GitHub Release automatically.
