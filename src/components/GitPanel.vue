<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { FileDiff, parsePatchFiles, type FileDiffMetadata } from "@pierre/diffs";
import { DialogClose, DialogContent, DialogOverlay, DialogPortal, DialogRoot, DialogTitle, DialogTrigger } from "reka-ui";
import { GitBranch, GitCommitHorizontal, Plus, RefreshCw, Undo2 } from "lucide-vue-next";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

type GitChange = {
  path: string;
  status: string;
  staged: boolean;
  unstaged: boolean;
  untracked: boolean;
};

type GitStatusResponse = {
  repoPath: string;
  branch: string;
  ahead: number;
  behind: number;
  changes: GitChange[];
};

type GitBranchesResponse = {
  current: string;
  branches: string[];
};

const props = defineProps<{
  open: boolean;
  repoPath: string | null;
}>();

const loading = ref(false);
const loadingDiff = ref(false);
const error = ref<string | null>(null);
const status = ref<GitStatusResponse | null>(null);
const selectedPath = ref<string | null>(null);
const diffContainer = ref<HTMLDivElement | null>(null);
const lastRepoPath = ref<string | null>(null);
const actionPath = ref<string | null>(null);
const actionLoading = ref(false);
const commitLoading = ref(false);
const commitMessage = ref("");
const remoteLoading = ref(false);
const remoteAction = ref<"fetch" | "pull" | "push" | null>(null);
const branchLoading = ref(false);
const checkoutLoading = ref(false);
const branches = ref<string[]>([]);
const currentBranch = ref<string>("");
const branchModalOpen = ref(false);
const branchQuery = ref("");

let diffInstance: FileDiff | null = null;
let refreshInFlight = false;
let refreshQueued = false;
let diffToken = 0;

const stagedCount = computed(() => status.value?.changes.filter((change) => change.staged).length ?? 0);
const unstagedCount = computed(() => status.value?.changes.filter((change) => change.unstaged).length ?? 0);
const untrackedCount = computed(() => status.value?.changes.filter((change) => change.untracked).length ?? 0);
const aheadCount = computed(() => status.value?.ahead ?? 0);
const behindCount = computed(() => status.value?.behind ?? 0);
const stagedChanges = computed(() => status.value?.changes.filter((change) => change.staged) ?? []);
const unstagedChanges = computed(
  () => status.value?.changes.filter((change) => change.unstaged || change.untracked) ?? [],
);
const pushButtonLabel = computed(() => {
  if (remoteAction.value === "push") return "Pushing...";
  if (aheadCount.value > 0) return `Push (${aheadCount.value})`;
  return "Push";
});
const pullButtonLabel = computed(() => {
  if (remoteAction.value === "pull") return "Pulling...";
  if (behindCount.value > 0) return `Pull (${behindCount.value})`;
  return "Pull";
});
const commitDisabledReason = computed(() => {
  if (commitLoading.value) return "Committing changes";
  if (stagedCount.value === 0) return "Stage files before commit";
  if (commitMessage.value.trim().length === 0) return "Enter a commit message";
  return null;
});
const canCommit = computed(() => commitDisabledReason.value === null);
const commitButtonLabel = computed(() => {
  if (commitLoading.value) return "Committing...";
  return "Commit";
});
const selectedBranch = computed(() => currentBranch.value || status.value?.branch || "");
const filteredBranches = computed(() => {
  const query = branchQuery.value.trim().toLowerCase();
  if (!query) return branches.value;
  return branches.value.filter((branch) => branch.toLowerCase().includes(query));
});

function renderEmpty(message: string) {
  if (!diffContainer.value) return;
  diffContainer.value.textContent = message;
}

function renderDiffFromPatch(patch: string) {
  if (!diffContainer.value || !diffInstance) return;

  if (!patch.trim()) {
    renderEmpty("No diff");
    return;
  }

  try {
    diffContainer.value.textContent = "";
    const parsed = parsePatchFiles(patch);
    const fileDiff: FileDiffMetadata | undefined = parsed[0]?.files?.[0];

    if (!fileDiff) {
      renderEmpty("Cannot parse diff");
      return;
    }

    diffInstance.render({
      fileDiff,
      containerWrapper: diffContainer.value,
    });
  } catch {
    renderEmpty("Cannot render diff");
  }
}

async function loadDiff(path: string) {
  const target = status.value?.changes.find((change) => change.path === path);
  if (!target || !status.value?.repoPath) return;

  const currentDiffToken = ++diffToken;

  loadingDiff.value = true;

  try {
    const patch = await invoke<string>("git_diff", {
      repoPath: status.value.repoPath,
      path: target.path,
      staged: false,
      untracked: target.untracked,
    });

    if (currentDiffToken !== diffToken) return;
    renderDiffFromPatch(patch);
  } catch {
    if (currentDiffToken !== diffToken) return;
    renderEmpty("Failed to load diff");
  } finally {
    if (currentDiffToken === diffToken) {
      loadingDiff.value = false;
    }
  }
}

async function refresh() {
  if (refreshInFlight) {
    refreshQueued = true;
    return;
  }

  refreshInFlight = true;
  loading.value = true;
  error.value = null;

  try {
    const effectiveRepoPath = props.repoPath ?? lastRepoPath.value ?? null;
    const nextStatus = await invoke<GitStatusResponse>("git_status", {
      repoPath: effectiveRepoPath,
    });
    status.value = nextStatus;
    lastRepoPath.value = nextStatus.repoPath;
    await loadBranches(nextStatus.repoPath);

    if (!nextStatus.changes.length) {
      selectedPath.value = null;
      renderEmpty("Working tree clean");
      return;
    }

    const stillSelected = nextStatus.changes.some((change) => change.path === selectedPath.value);
    selectedPath.value = stillSelected ? selectedPath.value : nextStatus.changes[0]?.path ?? null;

    if (selectedPath.value) {
      await loadDiff(selectedPath.value);
    }
  } catch (e) {
    const message = e instanceof Error ? e.message : "Failed to load git status";
    error.value = message;
    renderEmpty(message);
  } finally {
    loading.value = false;
    refreshInFlight = false;

    if (refreshQueued) {
      refreshQueued = false;
      refresh().catch(() => undefined);
    }
  }
}

async function loadBranches(repoPath: string) {
  branchLoading.value = true;
  try {
    const response = await invoke<GitBranchesResponse>("git_branches", { repoPath });
    branches.value = response.branches;
    currentBranch.value = response.current;
  } catch {
    branches.value = [];
    currentBranch.value = "";
  } finally {
    branchLoading.value = false;
  }
}

function pickFile(path: string) {
  selectedPath.value = path;
  loadDiff(path).catch(() => undefined);
}

function activeRepoPath() {
  return status.value?.repoPath ?? lastRepoPath.value ?? null;
}

async function stageFile(path: string) {
  const repoPath = activeRepoPath();
  if (!repoPath) return;

  actionPath.value = path;
  actionLoading.value = true;
  error.value = null;

  try {
    await invoke("git_stage", { repoPath, path });
    await refresh();
  } catch (e) {
    error.value = e instanceof Error ? e.message : "Failed to stage file";
  } finally {
    actionLoading.value = false;
    actionPath.value = null;
  }
}

async function stageAllUnstaged() {
  const repoPath = activeRepoPath();
  if (!repoPath || actionLoading.value || !unstagedChanges.value.length) return;

  actionPath.value = "__all__";
  actionLoading.value = true;
  error.value = null;

  try {
    await invoke("git_stage_all", { repoPath });
    await refresh();
  } catch (e) {
    error.value = e instanceof Error ? e.message : "Failed to stage all changes";
  } finally {
    actionLoading.value = false;
    actionPath.value = null;
  }
}

async function unstageFile(path: string) {
  const repoPath = activeRepoPath();
  if (!repoPath) return;

  actionPath.value = path;
  actionLoading.value = true;
  error.value = null;

  try {
    await invoke("git_unstage", { repoPath, path });
    await refresh();
  } catch (e) {
    error.value = e instanceof Error ? e.message : "Failed to unstage file";
  } finally {
    actionLoading.value = false;
    actionPath.value = null;
  }
}

async function commitChanges(amend = false) {
  if (!canCommit.value) return;
  const repoPath = activeRepoPath();
  if (!repoPath) return;

  commitLoading.value = true;
  error.value = null;

  try {
    await invoke("git_commit", {
      repoPath,
      message: commitMessage.value.trim(),
      amend,
    });
    commitMessage.value = "";
    await refresh();
  } catch (e) {
    error.value = e instanceof Error ? e.message : "Failed to commit";
  } finally {
    commitLoading.value = false;
  }
}

async function runRemoteAction(action: "fetch" | "pull" | "push") {
  const repoPath = activeRepoPath();
  if (!repoPath || remoteLoading.value) return;

  remoteLoading.value = true;
  remoteAction.value = action;
  error.value = null;

  try {
    await invoke(`git_${action}`, { repoPath });
    await refresh();
  } catch (e) {
    error.value = e instanceof Error ? e.message : `Failed to ${action}`;
  } finally {
    remoteLoading.value = false;
    remoteAction.value = null;
  }
}

async function switchBranch(branch: string) {
  const repoPath = activeRepoPath();
  const target = branch.trim();
  if (!repoPath || !target || checkoutLoading.value) return;
  if (target === currentBranch.value) return;

  checkoutLoading.value = true;
  error.value = null;

  try {
    await invoke("git_checkout", { repoPath, branch: target });
    await refresh();
    branchModalOpen.value = false;
    branchQuery.value = "";
  } catch (e) {
    error.value = e instanceof Error ? e.message : "Failed to switch branch";
  } finally {
    checkoutLoading.value = false;
  }
}

onMounted(async () => {
  diffInstance = new FileDiff({
    theme: { dark: "pierre-dark", light: "pierre-light" },
    themeType: "dark",
    diffStyle: "unified",
    hunkSeparators: "simple",
    diffIndicators: "bars",
    disableBackground: false,
    overflow: "wrap",
    unsafeCSS: `
[data-overflow='wrap'] [data-code] {
  overflow: visible !important;
}
`,
  });

  renderEmpty("");
  await refresh();
});

onBeforeUnmount(() => {
  diffInstance?.cleanUp();
});

watch(
  () => props.open,
  (isOpen) => {
    if (!isOpen) return;
    if (!status.value) {
      refresh().catch(() => undefined);
      return;
    }

    if (selectedPath.value) {
      loadDiff(selectedPath.value).catch(() => undefined);
    }
  },
);

watch(
  () => props.repoPath,
  (nextPath) => {
    if (nextPath) {
      lastRepoPath.value = nextPath;
    }
    if (!props.open) return;
    if (!nextPath && !lastRepoPath.value) return;
    refresh().catch(() => undefined);
  },
);
</script>

<template>
  <aside class="git-panel" :class="{ collapsed: !open }">
    <header class="git-head">
      <div class="git-title">
        <GitBranch :size="14" />
        <span>git</span>
        <DialogRoot v-model:open="branchModalOpen">
          <DialogTrigger as-child>
            <button
              class="branch-trigger"
              type="button"
              :title="selectedBranch || status?.branch || '-'"
              :disabled="branchLoading || checkoutLoading || !branches.length"
            >
              {{ selectedBranch || status?.branch || "-" }}
            </button>
          </DialogTrigger>
          <DialogPortal>
            <DialogOverlay class="branch-overlay" />
            <DialogContent class="branch-modal">
              <DialogTitle class="branch-modal-title">Switch Branch</DialogTitle>
              <input
                v-model="branchQuery"
                class="branch-search"
                type="text"
                placeholder="Filter branches"
                :disabled="checkoutLoading"
              />
              <div class="branch-list">
                <button
                  v-for="branch in filteredBranches"
                  :key="branch"
                  class="branch-option"
                  type="button"
                  :class="{ active: branch === currentBranch }"
                  :disabled="checkoutLoading || branch === currentBranch"
                  @click="switchBranch(branch)"
                >
                  <span>{{ branch }}</span>
                  <span v-if="branch === currentBranch">Current</span>
                </button>
                <p v-if="!filteredBranches.length" class="branch-empty">No matching branches</p>
              </div>
              <div class="branch-modal-actions">
                <DialogClose as-child>
                  <button class="branch-close-btn" type="button">Close</button>
                </DialogClose>
              </div>
            </DialogContent>
          </DialogPortal>
        </DialogRoot>
      </div>
      <div class="git-head-actions">
        <button
          class="git-action-btn"
          type="button"
          title="Fetch from remote"
          aria-label="Fetch from remote"
          :disabled="remoteLoading"
          @click="runRemoteAction('fetch')"
        >
          {{ remoteAction === "fetch" ? "Fetching..." : "Fetch" }}
        </button>
        <button
          class="git-action-btn"
          type="button"
          :title="behindCount > 0 ? `Pull ${behindCount} incoming commit(s)` : 'Pull from remote'"
          aria-label="Pull from remote"
          :disabled="remoteLoading"
          @click="runRemoteAction('pull')"
        >
          {{ pullButtonLabel }}
        </button>
        <button
          class="git-action-btn"
          type="button"
          :title="aheadCount > 0 ? `Push ${aheadCount} local commit(s)` : 'Push to remote'"
          aria-label="Push to remote"
          :disabled="remoteLoading"
          @click="runRemoteAction('push')"
        >
          {{ pushButtonLabel }}
        </button>
        <button
          class="git-btn"
          type="button"
          :title="loading ? 'Refreshing git status' : 'Refresh git status'"
          :aria-label="loading ? 'Refreshing git status' : 'Refresh git status'"
          :disabled="loading || remoteLoading"
          @click="refresh"
        >
          <RefreshCw :size="13" :class="{ spinning: loading }" />
        </button>
      </div>
    </header>

    <div class="git-body">
      <div class="git-files">
        <div class="git-controls">
          <div class="git-stats">
            <span title="Staged files">S {{ stagedCount }}</span>
            <span title="Unstaged tracked files">U {{ unstagedCount }}</span>
            <span title="Untracked files">? {{ untrackedCount }}</span>
          </div>
          <div class="commit-row">
            <input
              v-model="commitMessage"
              class="commit-input"
              type="text"
              placeholder="Commit message"
              :disabled="commitLoading"
              @keydown.enter.prevent="commitChanges()"
            />
            <div class="commit-actions">
              <button
                class="commit-btn"
                type="button"
                :title="commitDisabledReason ?? 'Commit staged changes'"
                :aria-label="commitButtonLabel"
                :disabled="!canCommit || commitLoading"
                @click="commitChanges()"
              >
                <GitCommitHorizontal :size="12" />
                <span>{{ commitButtonLabel }}</span>
              </button>
              <button
                class="commit-amend-btn"
                type="button"
                title="Commit with amend"
                aria-label="Commit with amend"
                :disabled="!canCommit || commitLoading"
                @click="commitChanges(true)"
              >
                Commit with amend
              </button>
            </div>
          </div>
        </div>

        <div class="git-file-list">
          <div v-if="stagedChanges.length" class="file-group">
            <div class="group-head">
              <span>Staged ({{ stagedChanges.length }})</span>
            </div>
            <div
              v-for="change in stagedChanges"
              :key="`staged-${change.path}`"
              class="file-row"
              :class="{ active: change.path === selectedPath }"
              @click="pickFile(change.path)"
            >
              <span class="code">{{ change.status }}</span>
              <span class="path">{{ change.path }}</span>
              <span class="row-actions">
                <button
                  class="row-btn"
                  type="button"
                  title="Unstage file"
                  aria-label="Unstage file"
                  :disabled="actionLoading && actionPath === change.path"
                  @click.stop="unstageFile(change.path)"
                >
                  <Undo2 :size="12" />
                </button>
              </span>
            </div>
          </div>
          <div v-if="unstagedChanges.length" class="file-group">
            <div class="group-head">
              <span>Unstaged ({{ unstagedChanges.length }})</span>
              <button
                class="group-btn"
                type="button"
                title="Stage all unstaged changes"
                :disabled="actionLoading || !unstagedChanges.length"
                @click.stop="stageAllUnstaged"
              >
                <Plus :size="11" />
                <span>Stage all</span>
              </button>
            </div>
            <div
              v-for="change in unstagedChanges"
              :key="`unstaged-${change.path}`"
              class="file-row"
              :class="{ active: change.path === selectedPath }"
              @click="pickFile(change.path)"
            >
              <span class="code">{{ change.status }}</span>
              <span class="path">{{ change.path }}</span>
              <span class="row-actions">
                <button
                  class="row-btn"
                  type="button"
                  title="Stage file"
                  aria-label="Stage file"
                  :disabled="actionLoading && actionPath === change.path"
                  @click.stop="stageFile(change.path)"
                >
                  <Plus :size="12" />
                </button>
              </span>
            </div>
          </div>
        </div>

        <p v-if="!loading && !(status?.changes.length ?? 0)" class="empty">No changes</p>
        <p v-if="error" class="empty">{{ error }}</p>
      </div>

      <div class="git-diff-wrap">
        <div v-if="loadingDiff" class="diff-loading">Loading diff...</div>
        <div ref="diffContainer" class="git-diff" />
      </div>
    </div>
  </aside>
</template>

<style scoped>
.git-panel {
  width: 360px;
  border-left: 1px solid var(--border-0);
  display: grid;
  grid-template-rows: 44px minmax(0, 1fr);
  min-width: 0;
  min-height: 0;
  background: var(--surface-2);
  transition: width 180ms ease;
}

.collapsed {
  width: 0;
  border-left: 0;
  overflow: hidden;
}

.git-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  border-bottom: 1px solid var(--border-0);
  padding: 0 10px;
}

.git-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  text-transform: lowercase;
  min-width: 0;
  color: var(--text-1);
}

.branch-trigger {
  height: 22px;
  max-width: 140px;
  border: 1px solid var(--border-1);
  border-radius: 6px;
  background: var(--surface-3);
  color: var(--accent-text);
  padding: 0 8px;
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
  transition: border-color 120ms ease, background 120ms ease;
}

.branch-trigger:hover {
  border-color: var(--accent-dim);
  background: var(--surface-4);
}

.branch-trigger:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.git-btn {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.git-btn:hover {
  background: var(--surface-4);
  border-color: var(--border-1);
  color: var(--text-0);
}

.git-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.git-action-btn {
  height: 24px;
  padding: 0 8px;
  border-radius: 6px;
  border: 1px solid var(--border-1);
  background: var(--surface-3);
  font-size: 11px;
  line-height: 1;
  color: var(--text-1);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.git-action-btn:hover {
  border-color: var(--border-2);
  background: var(--surface-5);
  color: var(--text-0);
}

.git-action-btn:disabled,
.git-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* Branch modal */

.branch-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.6);
  z-index: 60;
}

.branch-modal {
  position: fixed;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  width: min(420px, calc(100vw - 24px));
  max-height: min(520px, calc(100vh - 24px));
  border: 1px solid var(--border-2);
  border-radius: 12px;
  background: var(--surface-3);
  padding: 12px;
  display: grid;
  grid-template-rows: auto auto minmax(0, 1fr) auto;
  gap: 10px;
  z-index: 61;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.55);
}

.branch-modal-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-0);
}

.branch-search {
  width: 100%;
  border: 1px solid var(--border-1);
  background: var(--surface-0);
  color: var(--text-0);
  border-radius: 6px;
  padding: 7px 9px;
  font-size: 12px;
  outline: none;
  transition: border-color 120ms ease;
}

.branch-search:focus {
  border-color: var(--accent-dim);
  box-shadow: 0 0 0 2px var(--accent-glow);
}

.branch-list {
  min-height: 120px;
  max-height: 320px;
  overflow: auto;
  border: 1px solid var(--border-1);
  border-radius: 8px;
}

.branch-option {
  width: 100%;
  border: 0;
  border-bottom: 1px solid var(--border-0);
  background: transparent;
  padding: 8px 10px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  text-align: left;
  font-size: 12px;
  color: var(--text-1);
  cursor: pointer;
}

.branch-option:hover {
  background: var(--surface-5);
  color: var(--text-0);
}

.branch-option.active {
  background: var(--accent-glow);
  color: var(--accent-text);
}

.branch-empty {
  margin: 10px;
  font-size: 12px;
  color: var(--text-2);
}

.branch-modal-actions {
  display: flex;
  justify-content: flex-end;
}

.branch-close-btn {
  height: 26px;
  border-radius: 6px;
  padding: 0 12px;
  cursor: pointer;
  font-size: 12px;
}

/* Git body */

.git-body {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.git-files {
  flex: 0 0 220px;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-bottom: 1px solid var(--border-0);
}

.git-controls {
  padding: 8px;
  border-bottom: 1px solid var(--border-0);
  display: grid;
  gap: 8px;
}

.git-stats {
  display: flex;
  gap: 6px;
  font-size: 11px;
}

.git-stats > span {
  padding: 1px 6px;
  border-radius: 4px;
  font-weight: 500;
}

.git-stats > span:nth-child(1) {
  background: rgba(152, 195, 121, 0.12);
  color: var(--green);
}

.git-stats > span:nth-child(2) {
  background: rgba(229, 192, 123, 0.12);
  color: var(--yellow);
}

.git-stats > span:nth-child(3) {
  background: rgba(91, 141, 239, 0.12);
  color: var(--accent-text);
}

.commit-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 6px;
  align-items: center;
}

.commit-input {
  width: 100%;
  border: 1px solid var(--border-1);
  background: var(--surface-3);
  color: var(--text-0);
  border-radius: 5px;
  padding: 3px 6px;
  font-size: 11px;
  min-width: 0;
  outline: none;
  transition: border-color 120ms ease;
}

.commit-input:focus {
  border-color: var(--accent-dim);
  box-shadow: 0 0 0 2px var(--accent-glow);
}

.commit-btn {
  min-width: 72px;
  height: 22px;
  border-radius: 5px;
  padding: 0 5px;
  gap: 5px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 11px;
  background: var(--accent-dim);
  border-color: var(--accent);
  color: #e8efff;
}

.commit-btn:hover {
  background: var(--accent);
  border-color: var(--accent);
  color: #ffffff;
}

.commit-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
  background: var(--surface-3);
  border-color: var(--border-1);
  color: var(--text-2);
}

.commit-actions {
  position: relative;
  display: inline-flex;
  align-items: center;
}

.commit-amend-btn {
  position: absolute;
  right: 0;
  top: calc(100% + 4px);
  z-index: 3;
  white-space: nowrap;
  height: 24px;
  padding: 0 8px;
  border-radius: 6px;
  font-size: 11px;
  opacity: 0;
  pointer-events: none;
  transform: translateY(-2px);
  transition: opacity 120ms ease, transform 120ms ease;
}

.commit-actions:hover .commit-amend-btn,
.commit-actions:focus-within .commit-amend-btn {
  opacity: 1;
  pointer-events: auto;
  transform: translateY(0);
}

.commit-amend-btn:disabled {
  opacity: 0;
  pointer-events: none;
}

/* File list */

.git-file-list {
  overflow: auto;
  min-height: 0;
}

.file-group + .file-group {
  border-top: 1px solid var(--border-1);
}

.group-head {
  position: sticky;
  top: 0;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 10px;
  background: var(--surface-3);
  border-bottom: 1px solid var(--border-0);
  font-size: 11px;
  font-weight: 600;
  color: var(--text-2);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.group-btn {
  height: 20px;
  border-radius: 5px;
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  font-weight: 400;
  text-transform: none;
  letter-spacing: 0;
  cursor: pointer;
}

.group-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.file-row {
  width: 100%;
  text-align: left;
  border: 0;
  border-bottom: 1px solid var(--border-0);
  border-left: 2px solid transparent;
  background: transparent;
  padding: 7px 10px 7px 8px;
  display: grid;
  grid-template-columns: 26px minmax(0, 1fr) auto;
  gap: 6px;
  cursor: pointer;
  align-items: center;
  transition: background 100ms ease, border-color 100ms ease;
}

.file-row:hover {
  background: var(--surface-4);
}

.file-row.active {
  background: var(--accent-glow);
  border-left-color: var(--accent);
}

.code {
  color: var(--accent-text);
  font-size: 11px;
  font-weight: 600;
  font-family: "JetBrains Mono Variable", monospace;
}

.path {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
  color: var(--text-1);
}

.file-row.active .path {
  color: var(--text-0);
}

.row-actions {
  display: inline-flex;
  gap: 4px;
}

.row-btn {
  width: 20px;
  height: 20px;
  border-radius: 5px;
  padding: 0;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.row-btn:hover {
  background: var(--surface-5);
  border-color: var(--border-1);
  color: var(--text-0);
}

.spinning {
  animation: spin 700ms linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.empty {
  margin: 10px;
  font-size: 12px;
  color: var(--text-2);
}

/* Diff */

.git-diff-wrap {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
}

.diff-loading {
  position: absolute;
  right: 10px;
  top: 8px;
  z-index: 2;
  font-size: 11px;
  color: var(--text-2);
}

.git-diff {
  min-height: 0;
}
</style>
