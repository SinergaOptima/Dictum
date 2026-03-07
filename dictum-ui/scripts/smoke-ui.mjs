import { chromium } from "playwright";

const baseUrl = process.env.DICTUM_SMOKE_URL ?? "http://127.0.0.1:3010";

function buildInitScript(config) {
  return `
    (() => {
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
          currentVersion: "0.1.8-dev.2",
          latestVersion: "0.1.8-dev.2",
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

      const diagnosticsBundle = {
        generatedAt: new Date().toISOString(),
        appVersion: "0.1.8-dev.2",
        updateRepoSlug: "sinergaoptima/dictum",
        settingsPath: "C:/Users/Test/AppData/Roaming/Dictum/settings.json",
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
        devices: [
          { name: "Microphone (USB)", isDefault: true, isLoopbackLike: false, sampleRate: 48000, channels: 1 }
        ],
        correctionDiagnostics: {
          totalRules: state.learnedCorrections.length,
          globalRules: 1,
          modeScopedRules: 0,
          profileScopedRules: 0,
          unusedRules: 0,
          topRules: state.learnedCorrections.map((r) => ({ ...r, appProfileName: null })),
          recentRules: state.learnedCorrections.map((r) => ({ ...r, appProfileName: null }))
        }
      };
      state.diagnosticsBundle = diagnosticsBundle;

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
              return "0.1.8-dev.2";
            case "plugin:event|listen":
              return Math.floor(Math.random() * 10000);
            case "plugin:event|unlisten":
              return null;
            case "get_preferred_input_device":
              return "Microphone (USB)";
            case "list_audio_devices":
              return diagnosticsBundle.devices;
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
              state.diagnosticsBundle.runtimeSettings = state.runtimeSettings;
              return state.runtimeSettings;
            case "upsert_app_profile": {
              const profile = args.profile;
              const idx = state.appProfiles.findIndex((p) => p.id === profile.id);
              if (idx >= 0) state.appProfiles[idx] = profile;
              else state.appProfiles.push(profile);
              return state.appProfiles;
            }
            case "delete_app_profile":
              state.appProfiles = state.appProfiles.filter((p) => p.id !== args.id);
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
              state.diagnosticsBundle.correctionDiagnostics.totalRules = state.learnedCorrections.length;
              return state.learnedCorrections;
            }
            case "delete_learned_correction":
              state.learnedCorrections = state.learnedCorrections.filter((r) =>
                !(r.heard === args.heard && r.corrected === args.corrected)
              );
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
      foregroundApp: null,
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
  await assertText(page, 'Failed to import app profiles: Profile 1: appMatch must be a Windows executable like "cursor.exe".');

  await page.getByRole("button", { name: /stats/i }).click();
  await assertText(page, "Release Readiness");
  await assertText(page, "Smoke Benchmark Baseline");
  await page.getByRole("button", { name: /copy checklist/i }).click();
  await page.getByRole("button", { name: /checklist copied/i }).waitFor({ timeout: 10000 });
  await page.getByRole("button", { name: /export file/i }).click();
  await assertText(page, "Diagnostics export");
  await assertText(page, "Exported");

  await page.close();
  await context.close();
}

async function main() {
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
  if (failures.length > 0) {
    console.error(failures.join("\n"));
    process.exit(1);
  }
  console.log("Smoke UI checks passed.");
}

await main();
