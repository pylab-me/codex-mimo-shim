# Changelog

## v0.1.9

- Support `reasoning_content` field ([passing-back-reasoning_content](https://platform.xiaomimimo.com/docs/zh-CN/usage-guide/passing-back-reasoning_content)).
  - [config.example.yaml](config/config.example.yaml) is updated.

## v0.1.8-dev.1

- Support Intel-based Macs.
  - Tested on Codex-desktop & bugfix some fields.
- Improve compatibility error handling; now explicitly returns "unsupported" only in stage-1.

## v0.1.7

- Support audio input (Note: This feature is not supported by XIAOMI-mimo).
- Fix image input bug ([#2](https://github.com/pylab-me/codex-mimo-shim/issues/2)).

## v0.1.6

- Initial public binary distribution repository skeleton.
- Add manual release workflow for closed-source binary releases.
- Add public `build.rs` metadata injector.
- Add release verification guide.
- Add security policy.
