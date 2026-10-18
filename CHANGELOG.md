# Changelog

## [0.5.1](https://github.com/microtak/microtak-admin-web/compare/v0.5.0...v0.5.1) (2026-10-18)


### Documentation

* code coverage in the README ([f545a69](https://github.com/microtak/microtak-admin-web/commit/f545a69c72835dac1a87b5820c6aa1074cf83f0e))
* when mission roles can be set ([2be3a42](https://github.com/microtak/microtak-admin-web/commit/2be3a42a94b146dd737851d5da8d143477d089ad))

## [0.5.0](https://github.com/microtak/microtak-admin-web/compare/v0.4.0...v0.5.0) (2026-10-17)


### ⚠ BREAKING CHANGES

* needs microtak-server 0.9.0.

### Features

* devices page with certificate revocation and identity blocks ([4f09543](https://github.com/microtak/microtak-admin-web/commit/4f0954333cd96e695f19a1485ab38d44d06283c2))
* mission pages use the official mission API ([9344775](https://github.com/microtak/microtak-admin-web/commit/93447752467ce13a5de778b6fe244ef6fe9080d6))


### Bug Fixes

* **deps:** maud 0.27, rustls 0.23.46 ([df447c6](https://github.com/microtak/microtak-admin-web/commit/df447c68baca096000a44a8ba1cf3b3b63b34696))
* devices page uses the official certificate admin API ([31d99ff](https://github.com/microtak/microtak-admin-web/commit/31d99ff28fc3cb10939b427dc8a0484a52d1a13c))
* refuse cross-site form posts (CSRF) ([62978b5](https://github.com/microtak/microtak-admin-web/commit/62978b5cb6c7ccf613468be6c68e6e47a825ff30))


### Documentation

* non-affiliation notice in the README ([e7d6285](https://github.com/microtak/microtak-admin-web/commit/e7d6285c133e240fa2199ee1f38bf448719cd0ef))
* SECURITY.md, README for the official mission API ([a7525e2](https://github.com/microtak/microtak-admin-web/commit/a7525e2415644a15bc6f6ba6ce8d29cc5da78711))

## [0.4.0](https://github.com/microtak/microtak-admin-web/compare/v0.3.0...v0.4.0) (2026-09-29)


### ⚠ BREAKING CHANGES

* the QR payload format changed; MICROTAK_ADMIN_WEB_ENROLLMENT_URL must be an https:// URL.

### Features

* groups (channels) page; groups on tokens; read-only mission role ([#6](https://github.com/microtak/microtak-admin-web/issues/6)) ([44b71b4](https://github.com/microtak/microtak-admin-web/commit/44b71b484b56e2429ea06914448d824edf041bfa))
* standard tak:// enrollment QR codes, tokens bound to a device ([#4](https://github.com/microtak/microtak-admin-web/issues/4)) ([efeff58](https://github.com/microtak/microtak-admin-web/commit/efeff58ed179ef9cf1d905f351c962712facaa1c))

## [0.3.0](https://github.com/microtak/microtak-admin-web/compare/v0.2.0...v0.3.0) (2026-09-24)


### Features

* add microtak-admin-web, a standalone web admin UI ([027e12e](https://github.com/microtak/microtak-admin-web/commit/027e12e5ef9c15f3ca6aa03cd3fd7e237f34ae24))


### Bug Fixes

* re-wrap bare-base64 signedCert into full PEM in e2e test helper ([64dc824](https://github.com/microtak/microtak-admin-web/commit/64dc824f7eb2f1e9695e722330ef9205aad2cf79))


### Documentation

* **readme:** update enrollment lockdown references to Auto/Open ([652b70b](https://github.com/microtak/microtak-admin-web/commit/652b70ba9dcbc35a3182b569e38311b0155d2133))

## [0.2.0](https://github.com/microtak/microtak-admin-web/compare/v0.1.0...v0.2.0) (2026-09-23)


### Features

* add microtak-admin-web, a standalone web admin UI ([027e12e](https://github.com/microtak/microtak-admin-web/commit/027e12e5ef9c15f3ca6aa03cd3fd7e237f34ae24))


### Documentation

* **readme:** update enrollment lockdown references to Auto/Open ([17ec34a](https://github.com/microtak/microtak-admin-web/commit/17ec34af7d742bfd3899ccfad452400a269e9e03))
