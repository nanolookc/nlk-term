<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { FitAddon, init, Terminal } from "ghostty-web";
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

const props = defineProps<{
  tabId: string;
  active: boolean;
}>();

const emit = defineEmits<{
  (e: "session-meta", payload: { tabId: string; shell: string }): void;
  (e: "session-title", payload: { tabId: string; title: string }): void;
}>();

const ghosttyReady = init();
const DEFAULT_FONT_SIZE = 16;
const MIN_FONT_SIZE = 10;
const MAX_FONT_SIZE = 32;

const root = ref<HTMLDivElement | null>(null);
let terminal: Terminal | null = null;
let fitAddon: FitAddon | null = null;
let unlistenData: UnlistenFn | null = null;
let unlistenExit: UnlistenFn | null = null;
let resizeDebounce: number | null = null;

function syncPtySize(cols: number, rows: number) {
  invoke("resize_terminal", {
    tabId: props.tabId,
    cols,
    rows,
  }).catch(() => undefined);
}

function fitTerminal(force = false) {
  if (!fitAddon || !terminal) return;
  if (!props.active && !force) return;

  fitAddon.fit();
  syncPtySize(terminal.cols, terminal.rows);
}

function setTerminalFontSize(next: number) {
  if (!terminal) return;

  const clamped = Math.max(MIN_FONT_SIZE, Math.min(MAX_FONT_SIZE, next));
  const options = terminal.options as unknown as { fontSize: number };

  if (options.fontSize === clamped) return;
  options.fontSize = clamped;
  fitTerminal(true);
}

onMounted(async () => {
  await ghosttyReady;

  terminal = new Terminal({
    cursorBlink: true,
    fontFamily: '"JetBrains Mono Variable", "JetBrains Mono", Menlo, monospace',
    fontSize: DEFAULT_FONT_SIZE,
    theme: {
      background: "#0b0b0b",
      foreground: "#ededed",
      cursor: "#d7d7d7",
      cursorAccent: "#0b0b0b",
      selectionBackground: "#2f3540",
      selectionForeground: "#f5f5f5",
      black: "#1b1b1b",
      red: "#f07178",
      green: "#aad94c",
      yellow: "#ffcb6b",
      blue: "#82aaff",
      magenta: "#c792ea",
      cyan: "#89ddff",
      white: "#d6deeb",
      brightBlack: "#5b6470",
      brightRed: "#ff8b92",
      brightGreen: "#c3e88d",
      brightYellow: "#ffd580",
      brightBlue: "#9cc4ff",
      brightMagenta: "#ddb6f2",
      brightCyan: "#b2ebff",
      brightWhite: "#ffffff",
    },
  });

  if (root.value) {
    terminal.open(root.value);
  }

  fitAddon = new FitAddon();
  terminal.loadAddon(fitAddon);

  terminal.onData((input) => {
    invoke("write_terminal", { tabId: props.tabId, data: input }).catch(() => undefined);
  });

  terminal.attachCustomKeyEventHandler((event) => {
    const withModifier = event.ctrlKey || event.metaKey;
    if (!withModifier) return false;

    const key = event.key;
    if (key === "+" || key === "=" || event.code === "NumpadAdd") {
      event.preventDefault();
      const options = terminal?.options as unknown as { fontSize: number };
      setTerminalFontSize(options.fontSize + 1);
      return true;
    }

    if (key === "-" || key === "_" || event.code === "NumpadSubtract") {
      event.preventDefault();
      const options = terminal?.options as unknown as { fontSize: number };
      setTerminalFontSize(options.fontSize - 1);
      return true;
    }

    if (key === "0" || event.code === "Numpad0") {
      event.preventDefault();
      setTerminalFontSize(DEFAULT_FONT_SIZE);
      return true;
    }

    return false;
  });

  terminal.onResize(({ cols, rows }) => {
    if (resizeDebounce !== null) {
      window.clearTimeout(resizeDebounce);
    }

    resizeDebounce = window.setTimeout(() => {
      syncPtySize(cols, rows);
      resizeDebounce = null;
    }, 24);
  });

  const onTitleChange = (terminal as unknown as { onTitleChange?: (cb: (title: string) => void) => void })
    .onTitleChange;
  if (onTitleChange) {
    onTitleChange((title) => {
      emit("session-title", { tabId: props.tabId, title });
    });
  }

  unlistenData = await listen<{ tabId: string; data: string }>("terminal-data", (event) => {
    if (event.payload.tabId === props.tabId) {
      terminal?.write(event.payload.data);
    }
  });

  unlistenExit = await listen<{ tabId: string }>("terminal-exit", (event) => {
    if (event.payload.tabId === props.tabId) {
      terminal?.writeln("\r\n[process exited]");
    }
  });

  const session = await invoke<{ shell: string }>("open_terminal", { tabId: props.tabId });
  emit("session-meta", { tabId: props.tabId, shell: session.shell });

  fitTerminal(true);
  fitAddon.observeResize();

  if (props.active) {
    terminal.focus();
  }
});

onBeforeUnmount(async () => {
  if (resizeDebounce !== null) {
    window.clearTimeout(resizeDebounce);
  }

  if (unlistenData) unlistenData();
  if (unlistenExit) unlistenExit();

  await invoke("close_terminal", { tabId: props.tabId }).catch(() => undefined);
  terminal?.dispose();
});

watch(
  () => props.active,
  async (isActive) => {
    if (!terminal) return;

    if (isActive) {
      fitTerminal(true);
      terminal.focus();
    }
  },
);
</script>

<template>
  <div class="terminal-pane" :class="{ hidden: !active }">
    <div ref="root" class="terminal-root" />
  </div>
</template>

<style scoped>
.terminal-pane {
  position: absolute;
  inset: 2px;
  overflow: hidden;
}

.hidden {
  opacity: 0;
  pointer-events: none;
}

.terminal-root {
  width: 100%;
  height: 100%;
  overflow: hidden;
}
</style>
