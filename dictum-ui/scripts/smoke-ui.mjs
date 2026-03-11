import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright";

const baseUrl = process.env.DICTUM_SMOKE_URL ?? "http://127.0.0.1:3010";
const managedServer = !process.env.DICTUM_SMOKE_URL;
const exportDir = path.join(process.cwd(), "out");

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForServer(url, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url, { redirect: "manual" });
      if (response.ok || response.status === 307 || response.status === 308) {
        return;
      }
      lastError = new Error(`Unexpected HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await sleep(500);
  }
  throw lastError instanceof Error ? lastError : new Error("Server did not become ready.");
}

function contentTypeFor(filePath) {
  const ext = path.extname(filePath).toLowerCase();
  switch (ext) {
    case ".html":
      return "text/html; charset=utf-8";
    case ".js":
      return "application/javascript; charset=utf-8";
    case ".css":
      return "text/css; charset=utf-8";
    case ".json":
      return "application/json; charset=utf-8";
    case ".txt":
      return "text/plain; charset=utf-8";
    case ".svg":
      return "image/svg+xml";
    case ".png":
      return "image/png";
    case ".jpg":
    case ".jpeg":
      return "image/jpeg";
    case ".woff":
      return "font/woff";
    case ".woff2":
      return "font/woff2";
    default:
      return "application/octet-stream";
  }
}

async function resolveExportFile(urlPath) {
  const normalizedPath = decodeURIComponent(urlPath.split("?")[0]);
  const trimmed = normalizedPath.replace(/^\/+/, "");
  const candidates = [];
  if (!trimmed) {
    candidates.push("index.html");
  } else {
    candidates.push(trimmed);
    candidates.push(`${trimmed}.html`);
    candidates.push(path.join(trimmed, "index.html"));
  }

  for (const candidate of candidates) {
    const filePath = path.join(exportDir, candidate);
    try {
      const data = await readFile(filePath);
      return { filePath, data };
    } catch {
      // Try the next candidate.
    }
  }
  return null;
}

async function startManagedServer() {
  const server = createServer(async (req, res) => {
    const resolved = await resolveExportFile(req.url || "/");
    if (!resolved) {
      res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("Not found");
      return;
    }
    res.writeHead(200, { "Content-Type": contentTypeFor(resolved.filePath) });
    res.end(resolved.data);
  });

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(3010, "127.0.0.1", resolve);
  });
  await waitForServer(baseUrl, 5000);
  return server;
}

function buildInitScript(config) {
  return `
    (() => {
      const currentVersion = "0.1.9-dev.1";
      const state = {
        runtimeSettings: {
          modelProfile: "distil-large-v3",
          performanceProfile: "whisper_balanced_english",
          dictationMode: "conversation",
          toggleShortcut: "Ctrl+Shift+Space",
          ortEp: "auto",
          ortIntraThreads: 0,
          ortInterThreads: 0,
          ortParallel: true,
          languageHint: "english",
          pillVisualizerSensitivity: 10,
          activitySensitivity: 4.2,
          activityNoiseGate: 0.0015,
          activityClipThreshold: 0.32,
          inputGainBoost: 1,
          postUtteranceRefine: false,
          phraseBiasTerms: [],
          hasOpenAiApiKey: false,
          cloudMode: "local_only",
          cloudOptIn: false,
          reliabilityMode: true,
          onboardingCompleted: ${config.onboardingCompleted ? "true" : "false"},
          historyEnabled: true,
          retentionDays: 90,
          correctionCount: 0,
          appProfileCount: 1
        },
        privacySettings: {
          historyEnabled: true,
          retentionDays: 90,
          cloudOptIn: false
        },
        recommendation: {
          recommendedProfile: "large-v3-turbo",
          suggestedOrtEp: "cpu",
          reason: "Smoke recommendation"
        },
        catalog: [
          { profile: "distil-large-v3", label: "Distil Large v3", speedTier: "fast", qualityTier: "good", minRamGb: 4, minVramGb: null, englishOptimized: true, notes: "" },
          { profile: "large-v3-turbo", label: "Large v3 Turbo", speedTier: "balanced", qualityTier: "high", minRamGb: 8, minVramGb: null, englishOptimized: true, notes: "" }
        ],
        activeAppContext: ${JSON.stringify(config.activeAppContext)},
        appProfiles: [
          {
            id: "cursor-profile",
            name: "Cursor",
            appMatch: "cursor.exe",
            dictationMode: "coding",
            phraseBiasTerms: ["TypeScript", "React"],
            postUtteranceRefine: true,
            enabled: true
          }
        ],
        learnedCorrections: [
          {
            heard: "ladder labs",
            corrected: "Lattice Labs",
            hits: 3,
            modeAffinity: null,
            appProfileAffinity: null,
            lastUsedAt: "2026-03-07T12:00:00Z"
          },
          {
            heard: "ship it",
            corrected: "ShipIt",
            hits: 1,
            modeAffinity: "command",
            appProfileAffinity: "missing-profile",
            lastUsedAt: null
          },
          {
            heard: "slash deploy",
            corrected: "/deploy",
            hits: 1,
            modeAffinity: "command",
            appProfileAffinity: null,
            lastUsedAt: "2025-10-01T10:00:00Z"
          }
        ],
        perfSnapshot: {
          diagnostics: {
            injectCalls: 0, injectSuccess: 0, finalSegmentsSeen: 0, fallbackStubTyped: 0,
            duplicateFinalSuppressed: 0, partialRescuesUsed: 0, shortcutToggleExecuted: 0,
            shortcutToggleDropped: 0, pipelineFramesIn: 0, pipelineFramesResampled: 0,
            pipelineVadWindows: 0, pipelineVadSpeech: 0, pipelineInferenceCalls: 0,
            pipelineInferenceErrors: 0, pipelineSegmentsEmitted: 0, pipelineFallbackEmitted: 0,
            pipelineDrainMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
            pipelineResampleMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
            pipelineVadMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
            pipelineInferenceMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 }
          },
          drainMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
          resampleMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
          vadMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
          inferenceMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
          transformMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
          injectMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
          persistMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 },
          finalizeMs: { count: 0, meanMs: 0, p50Ms: 0, p95Ms: 0, p99Ms: 0, maxMs: 0 }
        },
        stats: {
          rangeDays: 30,
          totalUtterances: 12,
          totalWords: 320,
          totalChars: 1800,
          avgLatencyMs: 850,
          buckets: []
        },
        historyPage: { items: [], total: 0, page: 1, pageSize: 50 },
        diagnosticsBundle: null,
        updateInfo: {
          currentVersion,
          latestVersion: currentVersion,
          hasUpdate: false,
          repoSlug: "sinergaoptima/dictum",
          releaseName: null,
          releaseNotes: null,
          publishedAt: null,
          htmlUrl: "https://github.com/SinergaOptima/Dictum",
          assetName: null,
          assetDownloadUrl: null,
          checksumAssetName: null,
          checksumAssetDownloadUrl: null,
          expectedInstallerSha256: null
        }
      };

      const devices = [
        { name: "Microphone (USB)", isDefault: true, isLoopbackLike: false, sampleRate: 48000, channels: 1 }
      ];

      const buildCorrectionDiagnostics = () => {
        const profileNameById = new Map(state.appProfiles.map((profile) => [profile.id, profile.name]));
        const summarizeRule = (rule) => ({
          ...rule,
          appProfileName: rule.appProfileAffinity ? profileNameById.get(rule.appProfileAffinity) || null : null
        });
        const topRules = [...state.learnedCorrections]
          .sort((a, b) => (b.hits - a.hits) || String(b.lastUsedAt || "").localeCompare(String(a.lastUsedAt || "")))
          .slice(0, 12)
          .map(summarizeRule);
        const recentRules = [...state.learnedCorrections]
          .filter((rule) => !!rule.lastUsedAt)
          .sort((a, b) => String(b.lastUsedAt || "").localeCompare(String(a.lastUsedAt || "")))
          .slice(0, 8)
          .map(summarizeRule);
        const ninetyDaysMs = 90 * 24 * 60 * 60 * 1000;
        return {
          totalRules: state.learnedCorrections.length,
          globalRules: state.learnedCorrections.filter((rule) => !rule.modeAffinity && !rule.appProfileAffinity).length,
          modeScopedRules: state.learnedCorrections.filter((rule) => !!rule.modeAffinity && !rule.appProfileAffinity).length,
          profileScopedRules: state.learnedCorrections.filter((rule) => !!rule.appProfileAffinity).length,
          unusedRules: state.learnedCorrections.filter((rule) => rule.hits <= 1 && !rule.lastUsedAt).length,
          orphanedProfileRules: state.learnedCorrections.filter((rule) => rule.appProfileAffinity && !profileNameById.has(rule.appProfileAffinity)).length,
          staleRules: state.learnedCorrections.filter((rule) => {
            if (rule.hits > 2 || !rule.lastUsedAt) return false;
            const ageMs = Date.now() - new Date(rule.lastUsedAt).getTime();
            return Number.isFinite(ageMs) && ageMs >= ninetyDaysMs;
          }).length,
          topRules,
          recentRules
        };
      };

      const refreshDiagnostics = () => {
        state.diagnosticsBundle = {
          generatedAt: new Date().toISOString(),
          appVersion: currentVersion,
          updateRepoSlug: "sinergaoptima/dictum",
          settingsPath: "C:/Users/Test/AppData/Roaming/Dictum/settings.json",
          settingsHealth: {
            loadedSchemaVersion: 0,
            currentSchemaVersion: 1,
            migrationApplied: true,
            migrationNotes: [
              "Legacy settings file had no schema version marker.",
              "Settings values were normalized on load to match current app rules."
            ]
          },
          activeAppContext: state.activeAppContext,
          runtimeSettings: state.runtimeSettings,
          privacySettings: state.privacySettings,
          perfSnapshot: state.perfSnapshot,
          historyStorage: {
            dbPath: "C:/Users/Test/AppData/Roaming/Dictum/history.db",
            totalRecords: 0,
            oldestCreatedAt: null,
            newestCreatedAt: null
          },
          devices,
          correctionDiagnostics: buildCorrectionDiagnostics()
        };
      };

      refreshDiagnostics();

      const listeners = new Map();
      let callbackId = 0;
      window.__DICTUM_SMOKE__ = state;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: () => {}
      };
      window.__TAURI_INTERNALS__ = {
        transformCallback: (cb) => {
          callbackId += 1;
          listeners.set(callbackId, cb);
          return callbackId;
        },
        unregisterCallback: (id) => listeners.delete(id),
        convertFileSrc: (path) => path,
        invoke: async (cmd, args = {}) => {
          switch (cmd) {
            case "plugin:app|version":
              return currentVersion;
            case "plugin:event|listen":
              return Math.floor(Math.random() * 10000);
            case "plugin:event|unlisten":
              return null;
            case "get_preferred_input_device":
              return "Microphone (USB)";
            case "list_audio_devices":
              return devices;
            case "get_runtime_settings":
              return state.runtimeSettings;
            case "get_privacy_settings":
              return state.privacySettings;
            case "get_model_profile_catalog":
              return state.catalog;
            case "get_model_profile_recommendation":
              return state.recommendation;
            case "get_app_profiles":
              return state.appProfiles;
            case "get_active_app_context":
              return state.activeAppContext;
            case "get_learned_corrections":
              return state.learnedCorrections;
            case "get_perf_snapshot":
              return state.perfSnapshot;
            case "get_stats":
              return state.stats;
            case "get_history":
              return state.historyPage;
            case "get_dictionary":
            case "get_snippets":
              return [];
            case "get_diagnostics_bundle":
              return state.diagnosticsBundle;
            case "export_diagnostics_bundle":
              return {
                path: "C:/Users/Test/AppData/Roaming/Dictum/diagnostics/dictum-diagnostics-smoke.json",
                fileName: "dictum-diagnostics-smoke.json"
              };
            case "check_for_app_update":
              return state.updateInfo;
            case "set_runtime_settings":
              state.runtimeSettings = { ...state.runtimeSettings, ...Object.fromEntries(Object.entries(args).filter(([, v]) => v !== null)) };
              refreshDiagnostics();
              return state.runtimeSettings;
            case "upsert_app_profile": {
              const profile = args.profile;
              const idx = state.appProfiles.findIndex((p) => p.id === profile.id);
              if (idx >= 0) state.appProfiles[idx] = profile;
              else state.appProfiles.push(profile);
              refreshDiagnostics();
              return state.appProfiles;
            }
            case "delete_app_profile":
              state.appProfiles = state.appProfiles.filter((p) => p.id !== args.id);
              refreshDiagnostics();
              return state.appProfiles;
            case "learn_correction": {
              state.learnedCorrections.push({
                heard: args.heard,
                corrected: args.corrected,
                hits: 1,
                modeAffinity: args.modeAffinity,
                appProfileAffinity: args.appProfileAffinity,
                lastUsedAt: new Date().toISOString()
              });
              refreshDiagnostics();
              return state.learnedCorrections;
            }
            case "prune_learned_corrections": {
              let removedUnused = 0;
              let removedOrphanedProfiles = 0;
              let removedStale = 0;
              const profileIds = new Set(state.appProfiles.map((profile) => profile.id));
              const ninetyDaysMs = 90 * 24 * 60 * 60 * 1000;
              state.learnedCorrections = state.learnedCorrections.filter((rule) => {
                const isUnused = !!args.removeUnused && rule.hits <= 1 && !rule.lastUsedAt;
                const isOrphaned =
                  !!args.removeOrphanedProfiles &&
                  !!rule.appProfileAffinity &&
                  !profileIds.has(rule.appProfileAffinity);
                const isStale =
                  !!args.removeStale &&
                  rule.hits <= 2 &&
                  !!rule.lastUsedAt &&
                  Number.isFinite(Date.now() - new Date(rule.lastUsedAt).getTime()) &&
                  Date.now() - new Date(rule.lastUsedAt).getTime() >= ninetyDaysMs;
                if (isUnused) removedUnused += 1;
                if (isOrphaned) removedOrphanedProfiles += 1;
                if (isStale) removedStale += 1;
                return !(isUnused || isOrphaned || isStale);
              });
              refreshDiagnostics();
              return {
                rules: state.learnedCorrections,
                removedUnused,
                removedOrphanedProfiles,
                removedStale
              };
            }
            case "delete_learned_correction":
              state.learnedCorrections = state.learnedCorrections.filter((r) =>
                !(r.heard === args.heard && r.corrected === args.corrected)
              );
              refreshDiagnostics();
              return state.learnedCorrections;
            default:
              return null;
          }
        }
      };
    })();
  `;
}

async function assertText(page, text) {
  await page.getByText(text, { exact: false }).waitFor({ timeout: 10000 });
}

async function waitForInputValue(locator, expected, timeoutMs = 10000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if ((await locator.inputValue()) === expected) {
      return;
    }
    await sleep(150);
  }
  throw new Error(`Expected input value "${expected}", got "${await locator.inputValue()}".`);
}

async function runOnboardingSmoke(browser) {
  const context = await browser.newContext();
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: baseUrl });
  const page = await context.newPage();
  await page.addInitScript(buildInitScript({
    onboardingCompleted: false,
    activeAppContext: {
      foregroundApp: null,
      matchedProfileId: null,
      matchedProfileName: null,
      dictationMode: "conversation",
      phraseBiasTermCount: 0,
      postUtteranceRefine: false
    }
  }));
  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  await assertText(page, "Welcome to Dictum");
  await page.getByRole("heading", { name: /guided voice tune/i }).waitFor({ timeout: 10000 });
  const finishButton = page.getByRole("button", { name: /finish setup/i });
  const disabled = await finishButton.isDisabled();
  if (!disabled) {
    throw new Error("Expected Finish Setup to stay disabled before guided tune completion.");
  }
  await page.close();
  await context.close();
}

async function runSettingsAndStatsSmoke(browser) {
  const context = await browser.newContext();
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: baseUrl });
  const page = await context.newPage();
  await page.addInitScript(buildInitScript({
    onboardingCompleted: true,
    activeAppContext: {
      foregroundApp: "notepad.exe",
      matchedProfileId: null,
      matchedProfileName: null,
      dictationMode: "conversation",
      phraseBiasTermCount: 0,
      postUtteranceRefine: false
    }
  }));
  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });

  await page.getByRole("button", { name: /settings/i }).click();
  await assertText(page, "Per-App Profiles");
  await assertText(page, "Live Corrections");
  await page.getByRole("button", { name: /use current app/i }).click();
  await page.getByLabel("App Match").fill("");
  await page.getByRole("button", { name: /use current app/i }).click();
  const currentAppMatch = await page.getByLabel("App Match").inputValue();
  if (currentAppMatch !== "notepad.exe") {
    throw new Error(`Expected Use Current App to prefill notepad.exe, got ${currentAppMatch || "<empty>"}.`);
  }
  await page.getByRole("button", { name: /^duplicate$/i }).first().evaluate((button) => button.click());
  await waitForInputValue(page.getByLabel("Profile Name"), "Cursor Copy");
  await waitForInputValue(page.getByLabel("App Match"), "");

  const profileScopeButton = page.getByRole("button", { name: /active profile/i });
  if (!(await profileScopeButton.isDisabled())) {
    throw new Error("Expected Active Profile correction scope to be disabled without a matched profile.");
  }

  const correctionImport = page.getByPlaceholder('[{"heard":"ladder labs","corrected":"Lattice Labs","hits":1}]');
  await correctionImport.fill('[{"heard":"","corrected":"x"}]');
  await page.getByRole("button", { name: /import corrections/i }).click();
  await assertText(page, "Failed to import learned corrections: Correction 1: missing heard text.");

  const profileImport = page.getByPlaceholder('[{"name":"Cursor Coding","appMatch":"cursor.exe","dictationMode":"coding","phraseBiasTerms":["TypeScript"],"postUtteranceRefine":true,"enabled":true}]');
  await profileImport.fill('[{"name":"Bad Profile","appMatch":"cursor","dictationMode":"coding"}]');
  await page.getByRole("button", { name: /import profiles/i }).click();
  await assertText(page, 'Failed to import app profiles: Profile 1: appMatch must resolve to a Windows executable like "cursor.exe".');

  await profileImport.fill('[{"id":"dup-profile","name":"Cursor One","appMatch":"cursor.exe","dictationMode":"coding"},{"id":"dup-profile","name":"Cursor Two","appMatch":"slack.exe","dictationMode":"conversation"}]');
  await page.getByRole("button", { name: /import profiles/i }).click();
  await assertText(page, 'Failed to import app profiles: Duplicate profile id "dup-profile" found in imported profiles.');

  await profileImport.fill('[{"name":"Slack Messaging","appMatch":"C:\\\\Program Files\\\\Slack\\\\slack.exe","dictationMode":"conversation","phraseBiasTerms":["Dictum"],"postUtteranceRefine":true,"enabled":true}]');
  await page.getByRole("button", { name: /import profiles/i }).click();
  await assertText(page, "Imported 1 app profile.");
  await page.getByText("slack.exe", { exact: true }).waitFor({ timeout: 10000 });

  await correctionImport.fill('[{"heard":"ship it","corrected":"ShipIt","hits":1,"appProfileAffinity":"missing-profile"}]');
  await page.getByRole("button", { name: /import corrections/i }).click();
  await assertText(page, 'Failed to import learned corrections: Correction 1: appProfileAffinity "missing-profile" does not match any saved app profile.');

  await correctionImport.fill('[{"heard":"slash deploy","corrected":"/deploy","modeAffinity":"command"},{"heard":"slash deploy","corrected":"/deploy","modeAffinity":"command"}]');
  await page.getByRole("button", { name: /import corrections/i }).click();
  await assertText(page, 'Failed to import learned corrections: Correction 2: duplicate scoped correction rule "slash deploy" -> "/deploy".');

  await assertText(page, "1 orphaned");
  await page.getByRole("button", { name: /prune orphaned/i }).click();
  await assertText(page, "Pruned 1 correction rule");
  await assertText(page, "0 orphaned");

  await page.getByRole("button", { name: /stats/i }).click();
  await assertText(page, "Release Readiness");
  await assertText(page, "Smoke Benchmark Baseline");
  await page.getByRole("heading", { name: /settings health/i }).waitFor({ timeout: 10000 });
  await page.getByText("Loaded Schema", { exact: true }).waitFor({ timeout: 10000 });
  await page.getByRole("button", { name: /copy checklist/i }).click();
  await page.getByRole("button", { name: /checklist copied/i }).waitFor({ timeout: 10000 });
  await page.getByRole("button", { name: /export file/i }).click();
  await assertText(page, "Diagnostics export");
  await assertText(page, "Exported");

  await page.close();
  await context.close();
}

async function main() {
  let server = null;
  if (managedServer) {
    server = await startManagedServer();
  }
  const browser = await chromium.launch({ headless: true });
  const failures = [];
  try {
    await runOnboardingSmoke(browser);
  } catch (error) {
    failures.push(`Onboarding smoke: ${error instanceof Error ? error.message : String(error)}`);
  }
  try {
    await runSettingsAndStatsSmoke(browser);
  } catch (error) {
    failures.push(`Settings/stats smoke: ${error instanceof Error ? error.message : String(error)}`);
  }
  await browser.close();
  if (server) {
    await new Promise((resolve) => server.close(resolve));
  }
  if (failures.length > 0) {
    console.error(failures.join("\n"));
    process.exit(1);
  }
  console.log("Smoke UI checks passed.");
}

await main();
