# Security policy

`covers` handles API credentials, so security reports are welcome.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub:
[**Report a vulnerability**](https://github.com/zunhdev/album-covers/security/advisories/new)
(the *Security* tab, then *Report a vulnerability*). Please don't open a public issue.

Include what you found, how to reproduce it, and which version (`covers --version`) is affected.
You can expect a first answer within a week.

## Supported versions

Only the latest release, or the `main` branch, gets fixes.

## Your credentials

`covers` reads TIDAL and Spotify client credentials from the environment, `.env`, or the
per-user `credentials.env` file. It sends them only to the services' own token endpoints,
and never prints them. If you think a credential was exposed, revoke or rotate it in the
service's developer dashboard.
