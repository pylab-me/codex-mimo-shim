# README / 使用说明

## Quick start / 快速启动

After configuring `config.toml` and preparing `mimo.json`, you can start Codex with one of the following profiles.

配置好 `config.toml` 并准备好 `mimo.json` 后，可以使用下面任一 profile 启动 Codex。

> [https://platform.xiaomimimo.com](https://platform.xiaomimimo.com/)

### Xiaomi MiMo-V2.5-Pro / 小米 MiMo-V2.5-Pro

```bash
codex --profile xiaomi-pro
```

> [huggingface](https://huggingface.co/XiaomiMiMo/MiMo-V2.5-Pro)


### Xiaomi MiMo-V2.5 / 小米 MiMo-V2.5

```bash
codex --profile xiaomi
```

> [huggingface](https://huggingface.co/XiaomiMiMo/MiMo-V2.5)

If you want to verify that the profile is loaded correctly, start Codex with the selected profile and check whether the
model name matches the expected Xiaomi Mimo model.

如果你想确认 profile 是否正确加载，可以使用指定 profile 启动 Codex，并检查实际使用的模型名称是否符合预期的小米 Mimo 模型。

## Overview / 概览

This project provides configuration examples for using Xiaomi Mimo models with Codex-compatible profile settings.

本项目提供了在 Codex 兼容配置中使用小米 Mimo 模型的示例配置。

The configuration below defines two profiles:

下面的配置定义了两个 profile：

- `xiaomi-pro`: uses `mimo-v2.5-pro`
- `xiaomi`: uses `mimo-v2.5`

Both profiles require a local model catalog file named `mimo.json`.

两个 profile 都需要一个本地模型目录文件：`mimo.json`。

---

## config.toml

Add the following profiles to your Codex `config.toml`.

请将下面的 profile 配置添加到你的 Codex `config.toml` 中。

```toml
[profiles.xiaomi-pro]
name = "mimo-v2.5-pro"
model = "mimo-v2.5-pro"
model_provider = "xiaomi-mimo"
model_reasoning_effort = "low"
model_verbosity = "medium"
model_catalog_json = "${REPLACE_YOUR_USER_DIR}/.codex/model-catalogs/mimo.json"

[profiles.xiaomi]
name = "mimo-v2.5"
model = "mimo-v2.5"
model_provider = "xiaomi-mimo"
model_reasoning_effort = "low"
model_verbosity = "medium"
model_catalog_json = "${REPLACE_YOUR_USER_DIR}/.codex/model-catalogs/mimo.json"
```

---

## Path configuration / 路径配置

Replace `${REPLACE_YOUR_USER_DIR}` with your actual user directory.

请将 `${REPLACE_YOUR_USER_DIR}` 替换为你的真实用户目录。

Examples:

示例：

```text
macOS/Linux:
${HOME}/.codex/model-catalogs/mimo.json

Windows:
C:\Users\YourName\.codex\model-catalogs\mimo.json
```

For example, on macOS or Linux, the final path may look like this:

例如，在 macOS 或 Linux 上，最终路径可能类似于：

```toml
model_catalog_json = "/Users/your-name/.codex/model-catalogs/mimo.json"
```

On Windows, the final path may look like this:

在 Windows 上，最终路径可能类似于：

```toml
model_catalog_json = "C:\\Users\\YourName\\.codex\\model-catalogs\\mimo.json"
```

---

## About mimo.json / 关于 mimo.json

The `mimo.json` file provides model catalog metadata for the Xiaomi Mimo models.

`mimo.json` 文件用于为小米 Mimo 模型提供模型目录 metadata。

This is useful when Codex cannot find model metadata automatically and falls back to default metadata. Missing or
fallback metadata may affect model behavior, capability detection, or performance-related settings.

当 Codex 无法自动找到模型 metadata，并回退到默认 metadata 时，`mimo.json` 会很有用。缺失或回退的 metadata
可能会影响模型行为、能力识别或性能相关设置。

For background, see:

背景说明可参考：

[openai/codex issue #12100](https://github.com/openai/codex/issues/12100)

---

## Directory layout / 推荐目录结构

Recommended local directory layout:

推荐的本地目录结构如下：

```text
.codex/
  config.toml
  model-catalogs/
    mimo.json
```

Make sure the path in `model_catalog_json` points to the actual `mimo.json` file.

请确保 `model_catalog_json` 指向真实存在的 `mimo.json` 文件。

---

## Notes / 注意事项

After updating `config.toml` or `mimo.json`, restart Codex to make sure the new configuration is loaded.

更新 `config.toml` 或 `mimo.json` 后，建议重启 Codex，确保新配置被正确加载。

If Codex reports that model metadata cannot be found, check the following:

如果 Codex 提示找不到模型 metadata，请检查以下内容：

1. `model_catalog_json` points to the correct file.
2. `mimo.json` exists at that path.
3. The profile `model` value matches the model name defined in `mimo.json`.
4. The file path uses valid escaping on your operating system.

对应检查项：

1. `model_catalog_json` 是否指向正确文件。
2. `mimo.json` 是否真实存在。
3. profile 中的 `model` 名称是否与 `mimo.json` 中定义的模型名称一致。
4. 文件路径在当前操作系统中是否使用了正确的转义方式。