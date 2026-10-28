# Security

TideTerm is under active development with no stable release or guaranteed security response time. Fixes target the current `master`; older versions are not guaranteed backports.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting option in this repository's **Security** tab if it is available. Its availability must be checked on GitHub; this document does not imply it has been enabled.

If the private reporting button is unavailable, open an Issue asking the maintainer for a private reporting channel, without exploit details, credentials or affected host information. Share details only after a private channel has been agreed. Do not publish an unpatched exploit in an Issue or PR.

Include the affected commit, reproduction steps using isolated data, expected/actual behavior and impact. Do not include real passwords, private keys, tokens, terminal logs or personal data.

## Using TideTerm

Verify SSH host fingerprints, protect remembered credentials with a master password, and review commands before sending. Terminal output and imported files are untrusted. Optional AI functionality calls an external API: check its scope before sending content. `--safe-mode` is a startup option, not a sandbox or security boundary.

请勿公开披露尚未修复的漏洞。优先使用仓库 Security 页的私密报告入口；若未开放，先发不含漏洞细节的 Issue，请维护者提供私下渠道。
