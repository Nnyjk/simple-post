#!/usr/bin/env node

import { execFile } from 'node:child_process';
import { statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

export const WORKSPACE_BINARY_NAMES = Object.freeze(['simple-post', 'simple-post-mcp']);
export const DEFAULT_WORKSPACE_ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));

export class ArtifactResolutionError extends Error {
  constructor(binaryName, resolution) {
    const artifact = resolution.artifacts[binaryName];
    const candidates = artifact?.candidates ?? [];
    super(
      `缺少 E2E 二进制产物 "${binaryName}"；已检查：${candidates.join(', ') || '(无候选路径)'}。请先运行 cargo build --workspace --bins。`,
    );
    this.name = 'ArtifactResolutionError';
    this.binaryName = binaryName;
    this.resolution = resolution;
  }
}

export function binaryFileName(binaryName, platform = process.platform) {
  return platform === 'win32' ? `${binaryName}.exe` : binaryName;
}

function normalizeProfiles(profiles) {
  const normalized = [...new Set(profiles)];
  if (normalized.length === 0) {
    throw new TypeError('至少需要一个构建 profile。');
  }
  return normalized;
}

function isFile(candidate) {
  try {
    return statSync(candidate).isFile();
  } catch {
    return false;
  }
}

function conciseError(error) {
  const stderr = typeof error?.stderr === 'string' ? error.stderr.trim() : '';
  const message = stderr || (error instanceof Error ? error.message : String(error));
  return message.replace(/\s+/g, ' ').slice(0, 2_000);
}

export async function readCargoMetadata({
  workspaceRoot = DEFAULT_WORKSPACE_ROOT,
  env = process.env,
} = {}) {
  const manifestPath = join(workspaceRoot, 'Cargo.toml');
  try {
    const { stdout } = await execFileAsync(
      'cargo',
      ['metadata', '--format-version', '1', '--no-deps', '--manifest-path', manifestPath],
      {
        cwd: workspaceRoot,
        env,
        encoding: 'utf8',
        maxBuffer: 8 * 1024 * 1024,
        windowsHide: true,
      },
    );
    const metadata = JSON.parse(stdout);
    if (typeof metadata.target_directory !== 'string') {
      throw new Error('cargo metadata 未返回 target_directory。');
    }
    return {
      status: 'ok',
      targetDirectory: resolve(workspaceRoot, metadata.target_directory),
      workspaceRoot:
        typeof metadata.workspace_root === 'string'
          ? resolve(workspaceRoot, metadata.workspace_root)
          : null,
    };
  } catch (error) {
    return {
      status: 'unavailable',
      targetDirectory: null,
      workspaceRoot: null,
      error: conciseError(error),
    };
  }
}

export async function resolveTargetDirectory({
  workspaceRoot = DEFAULT_WORKSPACE_ROOT,
  env = process.env,
  cwd = process.cwd(),
} = {}) {
  const invocationCwd = resolve(cwd);
  const configuredTarget = env.CARGO_TARGET_DIR;
  if (configuredTarget) {
    return {
      // Cargo 文档规定相对 CARGO_TARGET_DIR 以启动命令的 cwd 为基准，
      // 不能错误地相对 workspaceRoot 解析。
      targetDirectory: resolve(invocationCwd, configuredTarget),
      source: 'CARGO_TARGET_DIR',
      metadata: {
        status: 'skipped',
        targetDirectory: null,
        workspaceRoot: null,
        reason: `CARGO_TARGET_DIR 已显式设置，按调用 cwd ${invocationCwd} 解析。`,
      },
    };
  }

  const metadata = await readCargoMetadata({ workspaceRoot, env });
  if (metadata.status === 'ok') {
    return {
      targetDirectory: metadata.targetDirectory,
      source: 'cargo_metadata',
      metadata,
    };
  }

  // metadata 不可用时不能再猜测旧的 workspace/target，否则可能静默使用
  // 过期的构建产物；这里显式失败，让调用方报告真实解析错误。
  const detail = metadata.error || '未知错误';
  const error = new Error(
    `无法解析 workspace 产物目录：cargo metadata 不可用（${detail}）。请设置 CARGO_TARGET_DIR 或安装 cargo。`,
  );
  error.code = 'CARGO_METADATA_UNAVAILABLE';
  error.metadata = metadata;
  throw error;
}

export async function resolveWorkspaceArtifacts({
  workspaceRoot = DEFAULT_WORKSPACE_ROOT,
  env = process.env,
  cwd = process.cwd(),
  profiles = ['debug'],
  binaryNames = WORKSPACE_BINARY_NAMES,
  platform = process.platform,
} = {}) {
  const root = resolve(workspaceRoot);
  const checkedProfiles = normalizeProfiles(profiles);
  const target = await resolveTargetDirectory({ workspaceRoot: root, env, cwd });
  const artifacts = {};

  for (const binaryName of binaryNames) {
    const fileName = binaryFileName(binaryName, platform);
    const candidates = checkedProfiles.map((profile) =>
      join(target.targetDirectory, profile, fileName),
    );
    const selectedIndex = candidates.findIndex((candidate) => isFile(candidate));
    artifacts[binaryName] = {
      name: binaryName,
      fileName,
      profile: selectedIndex === -1 ? null : checkedProfiles[selectedIndex],
      path: selectedIndex === -1 ? null : candidates[selectedIndex],
      candidates,
      exists: selectedIndex !== -1,
    };
  }

  return {
    workspaceRoot: root,
    targetDirectory: target.targetDirectory,
    targetDirectorySource: target.source,
    cargoMetadata: target.metadata,
    profiles: checkedProfiles,
    artifacts,
    missing: binaryNames.filter((binaryName) => !artifacts[binaryName].exists),
  };
}

export async function requireWorkspaceBinary(binaryName, options = {}) {
  const resolution = await resolveWorkspaceArtifacts(options);
  if (!resolution.artifacts[binaryName]?.exists) {
    throw new ArtifactResolutionError(binaryName, resolution);
  }
  return {
    ...resolution.artifacts[binaryName],
    targetDirectory: resolution.targetDirectory,
    targetDirectorySource: resolution.targetDirectorySource,
  };
}
