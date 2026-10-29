# Security policy

Please do not disclose suspected vulnerabilities in a public issue. Use GitHub's private vulnerability reporting for `vibememory/cli`, including reproduction steps and affected versions.

Never include real Vibe Memory credentials, account data, or private memory records in a report. Test against a disposable self-hosted instance where possible.

Vault access is a separate server-side permission. The CLI intentionally has no
command that prints a Vault value; `vault exec` injects selected values into one
child process. The child process can still log or exfiltrate its environment, so
only launch commands that are trusted to receive those values.
