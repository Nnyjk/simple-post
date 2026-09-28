#!/usr/bin/env node

import { execFile } from 'node:child_process';
import { mkdtempSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import {
  binaryFileName,
  DEFAULT_WORKSPACE_ROOT,
  resolveWorkspaceArtifacts,
  WORKSPACE_BINARY_NAMES,
} from './artifact-resolver.mjs';

const execFileAsync = promisify(execFile);
const COMMAND_TIMEOUT_MS = 10_000;

function parseProfiles(value) {
  const profiles = value
    .split(',')
    .map((profile) => profile.trim())
    .filter(Boolean);
  if (profiles.length === 0) {
    throw new TypeError('--profiles 至少需要一个 profile。');
  }
  return profiles;
}

function commandFailure(binaryName, flag, error) {
  const stderr = typeof error?.stderr === 'string' ? error.stderr.trim() : '';
  const stdout = typeof error?.stdout === 'string' ? error.stdout.trim() : '';
  const detail = stderr || stdout || (error instanceof Error ? error.message : String(error));
  return {
    name: binaryName,
    flag,
    ok: false,
    exitCode: typeof error?.code === 'number' ? error.code : null,
    stdout: stdout.slice(0, 2_000),
    stderr: stderr.slice(0, 2_000),
    error: detail.replace(/\s+/g, ' ').slice(0, 2_000),
  };
}

async function runProbe(binaryPath, flag, scratch, env) {
  try {
    const { stdout, stderr } = await execFileAsync(binaryPath, [flag], {
      cwd: scratch,
      env: { ...env, SIMPLE_POST_DATA_DIR: scratch },
      encoding: 'utf8',
      maxBuffer: 1024 * 1024,
      timeout: COMMAND_TIMEOUT_MS,
      windowsHide: true,
    });
    return {
      flag,
      ok: true,
      exitCode: 0,
      stdout: stdout.trim().slice(0, 2_000),
      stderr: stderr.trim().slice(0, 2_000),
      error: null,
    };
  } catch (error) {
    return commandFailure(binaryPath, flag, error);
  }
}

function validateProbeContent(artifact, version, help) {
  const errors = [];
  const versionOutput = version.ok ? version.stdout.trim() : '';
  const helpOutput = help.ok ? help.stdout.trim() : '';
  if (version.ok && !versionOutput) {
    errors.push('--version 输出为空。');
  } else if (version.ok && !versionOutput.toLowerCase().includes(artifact.name.toLowerCase())) {
    errors.push(`--version 输出未包含二进制名称 "${artifact.name}"。`);
  }
  if (help.ok && !helpOutput) {
    errors.push('--help 输出为空。');
  }
  return errors;
}

async function probeArtifact(artifact, env) {
  const scratch = mkdtempSync(join(tmpdir(), 'simple-post-e2e-preflight-'));
  try {
    const probes = await Promise.all([
      runProbe(artifact.path, '--version', scratch, env),
      runProbe(artifact.path, '--help', scratch, env),
    ]);
    const version = probes[0];
    const help = probes[1];
    const versionValue = version.ok ? version.stdout.split(/\r?\n/)[0] : null;
    const validationErrors = validateProbeContent(artifact, version, help);

    return {
      name: artifact.name,
      path: artifact.path,
      fileName: artifact.fileName,
      profile: artifact.profile,
      version: versionValue,
      versionProbe: version,
      helpProbe: help,
      validationErrors,
      ok: version.ok && help.ok && validationErrors.length === 0,
    };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

function platformSummary(platform) {
  return {
    os: platform,
    arch: process.arch,
    node: process.version,
    binaryExtension: platform === 'win32' ? '.exe' : '',
  };
}

function reportExitCode(report) {
  return report.ok ? 0 : 1;
}

function failureReport(error, { workspaceRoot, profiles, platform, env, cwd }) {
  const message = error instanceof Error ? error.message : String(error);
  const root = resolve(workspaceRoot ?? DEFAULT_WORKSPACE_ROOT);
  const invocationCwd = resolve(cwd ?? process.cwd());
  return {
    schemaVersion: 1,
    kind: 'simple-post-e2e-preflight',
    generatedAt: new Date().toISOString(),
    ok: false,
    workspaceRoot: root,
    targetDirectory: env?.CARGO_TARGET_DIR ? resolve(invocationCwd, env.CARGO_TARGET_DIR) : null,
    targetDirectorySource: env?.CARGO_TARGET_DIR ? 'CARGO_TARGET_DIR' : null,
    cargoMetadata: error?.metadata ?? { status: 'unavailable' },
    platform: platformSummary(platform),
    profiles,
    missing: WORKSPACE_BINARY_NAMES,
    artifacts: WORKSPACE_BINARY_NAMES.map((name) => ({
      name,
      path: null,
      fileName: binaryFileName(name, platform),
      candidates: [],
      profile: null,
      version: null,
      versionProbe: commandFailure(name, '--version', error),
      helpProbe: commandFailure(name, '--help', error),
      validationErrors: [message],
      ok: false,
    })),
    error: {
      name: error instanceof Error ? error.name : 'Error',
      code: error?.code ?? null,
      message,
    },
  };
}

export async function runPreflight({
  workspaceRoot,
  env = process.env,
  cwd = process.cwd(),
  profiles = ['debug'],
  platform = process.platform,
} = {}) {
  const resolution = await resolveWorkspaceArtifacts({
    workspaceRoot,
    env,
    cwd,
    profiles,
    platform,
  });
  const artifacts = [];

  for (const name of WORKSPACE_BINARY_NAMES) {
    const artifact = resolution.artifacts[name];
    if (artifact.exists) {
      artifacts.push(await probeArtifact(artifact, env));
    } else {
      artifacts.push({
        name,
        path: null,
        fileName: artifact.fileName,
        profile: null,
        // 缺失产物也必须保留所有候选路径，方便直接定位构建输出问题。
        candidates: artifact.candidates,
        version: null,
        versionProbe: commandFailure(name, '--version', new Error('binary missing')),
        helpProbe: commandFailure(name, '--help', new Error('binary missing')),
        validationErrors: ['binary missing'],
        ok: false,
      });
    }
  }

  const report = {
    schemaVersion: 1,
    kind: 'simple-post-e2e-preflight',
    generatedAt: new Date().toISOString(),
    ok: resolution.missing.length === 0 && artifacts.every((artifact) => probeOk(artifact)),
    workspaceRoot: resolution.workspaceRoot,
    targetDirectory: resolution.targetDirectory,
    targetDirectorySource: resolution.targetDirectorySource,
    cargoMetadata: resolution.cargoMetadata,
    platform: platformSummary(platform),
    profiles: resolution.profiles,
    missing: resolution.missing,
    artifacts,
  };

  return { report, exitCode: reportExitCode(report) };
}

function probeOk(artifact) {
  return Boolean(
    artifact.versionProbe?.ok && artifact.helpProbe?.ok && artifact.validationErrors?.length === 0,
  );
}

function parseArgs(argv) {
  const profiles = ['debug'];
  let workspaceRoot;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--profiles') {
      profiles.splice(0, profiles.length, ...parseProfiles(argv[++index] ?? ''));
    } else if (arg.startsWith('--profiles=')) {
      profiles.splice(0, profiles.length, ...parseProfiles(arg.slice('--profiles='.length)));
    } else if (arg === '--workspace-root') {
      workspaceRoot = argv[++index];
    } else if (arg.startsWith('--workspace-root=')) {
      workspaceRoot = arg.slice('--workspace-root='.length);
    } else {
      throw new TypeError(`未知参数：${arg}`);
    }
  }
  return { profiles, workspaceRoot };
}

async function main() {
  const options = {
    env: process.env,
    cwd: process.cwd(),
    profiles: ['debug'],
    platform: process.platform,
  };
  try {
    const parsed = parseArgs(process.argv.slice(2));
    options.workspaceRoot = parsed.workspaceRoot;
    options.profiles = parsed.profiles;
    const { report, exitCode } = await runPreflight(options);
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    process.exitCode = exitCode;
  } catch (error) {
    const report = failureReport(error, options);
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`[e2e:preflight] ${message}\n`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && pathEqualsMain(process.argv[1])) {
  main().catch((error) => {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`[e2e:preflight] ${message}\n`);
    process.exitCode = 1;
  });
}

function pathEqualsMain(argument) {
  const mainPath = fileURLToPath(import.meta.url);
  let argumentPath = resolve(argument);
  let resolvedMainPath = mainPath;
  try {
    // realpath 可消除 symlink/大小写路径差异；不存在时保留 resolve 结果。
    argumentPath = realpathSync(argumentPath);
    resolvedMainPath = realpathSync(resolvedMainPath);
  } catch {
    // main 参数可能尚未落盘；后续仍按绝对路径比较。
  }
  return process.platform === 'win32'
    ? argumentPath.toLowerCase() === resolvedMainPath.toLowerCase()
    : argumentPath === resolvedMainPath;
}
