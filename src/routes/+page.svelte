<script lang="ts">
  import { invoke, Channel } from "@tauri-apps/api/core";

  function getSystemLang(): string {
    const lang = navigator.language.toLowerCase().split("-")[0];
    const supported = ["zh", "en", "ja", "de", "fr", "es", "pt", "id"];
    return supported.includes(lang) ? lang : "zh";
  }

  let appKey = $state("");
  let accessKey = $state("");
  let sourceLang = $state("ja");
  let targetLang = $state(getSystemLang());
  let enableTts = $state(true);
  let speakerId = $state("zh_female_xiaoai_uranus_bigtts");
  let isRunning = $state(false);
  let status = $state("");
  let subtitles: Array<{ source: string; translation: string }> = $state([]);
  let currentSource = $state("");
  let currentTranslation = $state("");
  let showSettings = $state(false);
  let subtitleEl: HTMLElement;
  let wasAtBottom = $state(true);

  async function loadSettings() {
    try {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load("settings.json");
      appKey = (await store.get<string>("app_key")) || "";
      accessKey = (await store.get<string>("access_key")) || "";
      const sl = await store.get<string>("source_lang");
      const tl = await store.get<string>("target_lang");
      if (sl) sourceLang = sl;
      if (tl) targetLang = tl;
      // Show settings if no credentials
      if (!appKey || !accessKey) showSettings = true;
    } catch (e) {
      showSettings = true;
    }
  }

  async function saveSettings() {
    try {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load("settings.json");
      await store.set("app_key", appKey);
      await store.set("access_key", accessKey);
      await store.set("source_lang", sourceLang);
      await store.set("target_lang", targetLang);
      await store.save();
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  }

  function closeSettings() {
    if (appKey && accessKey) {
      showSettings = false;
      saveSettings();
    }
  }

  async function start() {
    if (!appKey || !accessKey) {
      showSettings = true;
      return;
    }

    await saveSettings();

    const onSubtitle = new Channel<{
      type: string;
      text?: string;
      is_final?: boolean;
      message?: string;
    }>();

    onSubtitle.onmessage = (event) => {
      switch (event.type) {
        case "source":
          if (event.text) currentSource = event.text;
          break;
        case "translation":
          if (event.is_final && event.text) {
            currentTranslation = event.text;
            if (currentSource || currentTranslation) {
              // Deduplicate: skip if translation text matches any of the last 10 entries
              const recent = subtitles.slice(-10);
              const isDup = recent.some(
                (s) => s.translation === currentTranslation
              );
              if (!isDup) {
                subtitles = [
                  ...subtitles.slice(-19),
                  { source: currentSource, translation: currentTranslation },
                ];
              }
              currentSource = "";
              currentTranslation = "";
            }
          } else if (event.text) {
            currentTranslation = event.text;
          }
          break;
        case "status":
          status = event.message || "";
          // Handle terminal states from backend
          if (event.message === "已停止" || event.message === "会话结束") {
            isRunning = false;
          }
          break;
        case "error":
          status = event.message || "未知错误";
          isRunning = false;
          break;
      }
    };

    try {
      isRunning = true;
      status = "";
      await invoke("start_interpretation", {
        onSubtitle,
        appKey,
        accessKey,
        sourceLanguage: sourceLang,
        targetLanguage: targetLang,
        enableTts: enableTts,
        speakerId: enableTts ? speakerId : "",
      });
    } catch (e) {
      status = `${e}`;
      isRunning = false;
    }
  }

  async function stop() {
    try {
      await invoke("stop_interpretation");
      isRunning = false;
      status = "";
    } catch (e) {
      status = `${e}`;
    }
  }

  function toggleRunning() {
    if (isRunning) stop();
    else start();
  }

  // Track scroll position for sticky auto-scroll
  function onSubtitleScroll() {
    if (!subtitleEl) return;
    const threshold = 50;
    wasAtBottom = subtitleEl.scrollHeight - subtitleEl.scrollTop - subtitleEl.clientHeight < threshold;
  }

  // Sticky auto-scroll: only scroll to bottom if user was already at the bottom.
  // This lets users scroll up to read history without being yanked back down.
  $effect(() => {
    if (subtitles.length || currentSource || currentTranslation) {
      if (wasAtBottom) {
        requestAnimationFrame(() => {
          if (subtitleEl) subtitleEl.scrollTop = subtitleEl.scrollHeight;
        });
      }
    }
  });

  $effect(() => {
    loadSettings();

    // Ensure audio capture is stopped when window closes
    const cleanup = () => {
      if (isRunning) {
        invoke("stop_interpretation").catch(() => {});
      }
    };
    window.addEventListener("beforeunload", cleanup);
    return () => window.removeEventListener("beforeunload", cleanup);
  });

  const languages = [
    { code: "zh", name: "中文" },
    { code: "en", name: "EN" },
    { code: "ja", name: "日本語" },
    { code: "de", name: "DE" },
    { code: "fr", name: "FR" },
    { code: "es", name: "ES" },
    { code: "pt", name: "PT" },
    { code: "id", name: "ID" },
  ];

  const voices = [
    { id: "zh_female_vv_uranus_bigtts", name: "女声 A" },
    { id: "zh_female_xiaoai_uranus_bigtts", name: "女声 B" },
    { id: "zh_male_jingqiangkanye_emo_mars_bigtts", name: "男声" },
  ];
</script>

<main>
  <!-- Top bar -->
  <header>
    <div class="lang-pair" class:disabled={isRunning}>
      <select bind:value={sourceLang} disabled={isRunning}>
        {#each languages as lang}
          <option value={lang.code}>{lang.name}</option>
        {/each}
      </select>
      <svg class="arrow-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M5 12h14M12 5l7 7-7 7" />
      </svg>
      <select bind:value={targetLang} disabled={isRunning}>
        {#each languages as lang}
          <option value={lang.code}>{lang.name}</option>
        {/each}
      </select>
    </div>
    <button class="icon-btn" onclick={() => (showSettings = true)} title="设置">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M12 15a3 3 0 100-6 3 3 0 000 6z" />
        <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 008.7 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 8.7a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9c.26.604.852.997 1.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" />
      </svg>
    </button>
  </header>

  <!-- Subtitle area -->
  <section class="subtitles" bind:this={subtitleEl} onscroll={onSubtitleScroll}>
    {#if subtitles.length === 0 && !currentSource && !currentTranslation && !isRunning}
      <div class="empty-state">
        <div class="empty-icon">
          <svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="1.5">
            <circle cx="24" cy="24" r="20" />
            <path d="M16 20c0-4.4 3.6-8 8-8s8 3.6 8 8v4c0 4.4-3.6 8-8 8s-8-3.6-8-8v-4z" />
            <path d="M24 32v4M20 36h8" />
          </svg>
        </div>
        <p>点击下方按钮开始同声传译</p>
      </div>
    {/if}
    {#each subtitles as pair}
      <div class="subtitle-pair">
        <p class="source">{pair.source}</p>
        <p class="translation">{pair.translation}</p>
      </div>
    {/each}
    {#if currentSource || currentTranslation}
      <div class="subtitle-pair current">
        <p class="source">{currentSource}</p>
        <p class="translation">{currentTranslation}</p>
      </div>
    {/if}
  </section>

  <!-- Bottom controls -->
  <footer>
    {#if status}
      <span class="status" class:error={status.length > 0 && !isRunning}>{status}</span>
    {/if}
    <button
      class="main-btn"
      class:running={isRunning}
      onclick={toggleRunning}
    >
      {#if isRunning}
        <div class="btn-inner running-inner">
          <svg viewBox="0 0 24 24" fill="currentColor">
            <rect x="7" y="7" width="10" height="10" rx="2" />
          </svg>
          <span>停止</span>
        </div>
      {:else}
        <div class="btn-inner">
          <div class="mic-dot"></div>
          <span>开始同传</span>
        </div>
      {/if}
    </button>
  </footer>

  <!-- Settings overlay -->
  {#if showSettings}
    <div class="overlay" onclick={closeSettings}>
      <div class="settings-panel" onclick={(e) => e.stopPropagation()}>
        <div class="settings-header">
          <h2>设置</h2>
          <button class="icon-btn close-btn" onclick={closeSettings} disabled={!appKey || !accessKey}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div class="settings-body">
          <div class="api-info">
            <p class="api-desc">需要填入<a href="https://console.volcengine.com/speech/service/10030" target="_blank">豆包同声传译 2.0 大模型</a>的凭证</p>
            <p class="api-note">新用户可免费获得 100 万 token</p>
          </div>

          <div class="field">
            <label for="app-key">APP ID</label>
            <input
              id="app-key"
              type="text"
              bind:value={appKey}
              placeholder="豆包同传 APP ID"
            />
          </div>
          <div class="field">
            <label for="access-key">Access Token</label>
            <input
              id="access-key"
              type="password"
              bind:value={accessKey}
              placeholder="豆包同传 Access Token"
            />
          </div>

          <div class="divider"></div>

          <div class="field">
            <div class="tts-header">
              <label>语音同传</label>
              <label class="toggle">
                <input type="checkbox" bind:checked={enableTts} />
                <span class="toggle-track"><span class="toggle-thumb"></span></span>
              </label>
            </div>
            {#if enableTts}
              <div class="voice-group">
                {#each voices as v}
                  <button
                    class="voice-chip"
                    class:active={speakerId === v.id}
                    onclick={() => (speakerId = v.id)}
                  >{v.name}</button>
                {/each}
              </div>
            {/if}
          </div>
        </div>

        <button class="save-btn" onclick={closeSettings} disabled={!appKey || !accessKey}>
          完成
        </button>

        <p class="copyright">by wangxin · <a href="https://github.com/wxkingstar/TransEcho" target="_blank">GitHub</a></p>
      </div>
    </div>
  {/if}
</main>

<style>
  :global(html), :global(body) {
    overflow: hidden;
    margin: 0;
    padding: 0;
  }

  :global(::-webkit-scrollbar) {
    display: none;
  }

  :root {
    --bg: #0a0a0f;
    --surface: #141419;
    --border: #1e1e26;
    --text: #e8e8ed;
    --text-muted: #6b6b7b;
    --accent: #e0e0e0;
    --accent-hover: #ffffff;
    --danger: #ff5050;
    --radius: 12px;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", sans-serif;
    font-size: 14px;
    color: var(--text);
    background: var(--bg);
  }

  main {
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* Header */
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  .lang-pair {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .lang-pair.disabled {
    opacity: 0.5;
    pointer-events: none;
  }

  .lang-pair select {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text);
    padding: 6px 10px;
    border-radius: 8px;
    font-size: 13px;
    cursor: pointer;
    outline: none;
  }

  .lang-pair select:focus {
    border-color: var(--accent);
  }

  .arrow-icon {
    width: 16px;
    height: 16px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .icon-btn {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 8px;
    padding: 4px;
    transition: color 0.2s, background 0.2s;
  }

  .icon-btn:hover {
    color: var(--text);
    background: var(--surface);
  }

  .icon-btn svg {
    width: 20px;
    height: 20px;
  }

  /* Subtitles */
  .subtitles {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
    scroll-behavior: smooth;
  }

  /* scrollbar hidden globally, subtitles still scrollable */

  .empty-state {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    gap: 16px;
  }

  .empty-icon svg {
    width: 48px;
    height: 48px;
    opacity: 0.3;
  }

  .empty-state p {
    font-size: 13px;
    margin: 0;
  }

  .subtitle-pair {
    padding: 10px 0;
  }

  .subtitle-pair + .subtitle-pair {
    border-top: 1px solid var(--border);
  }

  .subtitle-pair.current {
    opacity: 0.5;
  }

  .subtitle-pair p {
    margin: 0;
    line-height: 1.6;
  }

  .source {
    color: var(--text-muted);
    font-size: 13px;
  }

  .translation {
    color: var(--text);
    font-size: 15px;
    margin-top: 2px;
  }

  /* Footer */
  footer {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 16px;
    gap: 8px;
    border-top: 1px solid var(--border);
  }

  .status {
    font-size: 12px;
    color: var(--text-muted);
  }

  .status.error {
    color: var(--danger);
  }

  .main-btn {
    width: 100%;
    max-width: 200px;
    padding: 14px 24px;
    border-radius: 28px;
    border: 1px solid rgba(255, 255, 255, 0.15);
    background: rgba(255, 255, 255, 0.08);
    color: var(--text);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.2s;
    font-size: 15px;
    font-weight: 500;
  }

  .main-btn:hover {
    background: rgba(255, 255, 255, 0.14);
    border-color: rgba(255, 255, 255, 0.25);
  }

  .main-btn:active {
    transform: scale(0.98);
  }

  .main-btn.running {
    background: rgba(255, 80, 80, 0.1);
    border-color: rgba(255, 80, 80, 0.25);
    color: var(--danger);
  }

  .main-btn.running:hover {
    background: rgba(255, 80, 80, 0.2);
  }

  .btn-inner {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .btn-inner svg {
    width: 18px;
    height: 18px;
  }

  .mic-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #4ade80;
    flex-shrink: 0;
  }

  .running-inner svg {
    animation: pulse 1.5s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  /* Settings overlay */
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: flex-end;
    z-index: 100;
    animation: fadeIn 0.15s ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .settings-panel {
    width: 100%;
    background: var(--surface);
    border-radius: 16px 16px 0 0;
    padding: 24px;
    animation: slideUp 0.2s ease;
    max-height: 80vh;
    overflow-y: auto;
  }

  @keyframes slideUp {
    from { transform: translateY(100%); }
    to { transform: translateY(0); }
  }

  .settings-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
  }

  .settings-header h2 {
    font-size: 16px;
    font-weight: 600;
    margin: 0;
  }

  .close-btn:disabled {
    opacity: 0.2;
    pointer-events: none;
  }

  .settings-body {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .api-info {
    padding: 12px 14px;
    background: rgba(255, 255, 255, 0.04);
    border-radius: 10px;
    border: 1px solid var(--border);
  }

  .api-desc {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.5;
  }

  .api-desc a {
    color: var(--text);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .api-desc a:hover {
    color: var(--accent-hover);
  }

  .api-note {
    margin: 6px 0 0;
    font-size: 12px;
    color: #4ade80;
    opacity: 0.85;
  }

  .field label {
    display: block;
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 8px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .field input[type="text"],
  .field input[type="password"] {
    width: 100%;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg);
    color: var(--text);
    font-size: 15px;
    box-sizing: border-box;
    outline: none;
    transition: border-color 0.2s;
  }

  .field input:focus {
    border-color: var(--accent);
  }

  .divider {
    height: 1px;
    background: var(--border);
  }

  .tts-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0;
  }

  .tts-header label:first-child {
    margin-bottom: 0;
  }

  .voice-group {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }

  .voice-chip {
    flex: 1;
    padding: 10px 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg);
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    transition: all 0.2s;
    text-align: center;
  }

  .voice-chip:hover {
    border-color: var(--text-muted);
    color: var(--text);
  }

  .voice-chip.active {
    border-color: rgba(255, 255, 255, 0.3);
    background: rgba(255, 255, 255, 0.08);
    color: var(--text);
  }

  /* Toggle switch */
  .toggle {
    position: relative;
    cursor: pointer;
    display: flex;
    align-items: center;
  }

  .toggle input {
    display: none;
  }

  .toggle-track {
    width: 40px;
    height: 22px;
    background: var(--border);
    border-radius: 11px;
    position: relative;
    transition: background 0.2s;
  }

  .toggle input:checked + .toggle-track {
    background: var(--accent);
  }

  .toggle-thumb {
    width: 18px;
    height: 18px;
    background: white;
    border-radius: 50%;
    position: absolute;
    top: 2px;
    left: 2px;
    transition: transform 0.2s;
  }

  .toggle input:checked + .toggle-track .toggle-thumb {
    transform: translateX(18px);
  }

  .save-btn {
    width: 100%;
    margin-top: 24px;
    padding: 14px;
    border: none;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.1);
    color: var(--text);
    font-size: 15px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.2s;
  }

  .save-btn:hover {
    background: rgba(255, 255, 255, 0.16);
  }

  .save-btn:disabled {
    opacity: 0.3;
    pointer-events: none;
  }

  .copyright {
    margin: 16px 0 0;
    text-align: center;
    font-size: 11px;
    color: var(--text-muted);
    opacity: 0.5;
  }

  .copyright a {
    color: var(--text-muted);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .copyright a:hover {
    color: var(--text);
  }
</style>
