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
  border-left: 1px solid #252525;
  display: grid;
  grid-template-rows: 44px minmax(0, 1fr);
  min-width: 0;
  min-height: 0;
  background: #111;
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
  border-bottom: 1px solid #252525;
  padding: 0 10px;
}

.git-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  text-transform: lowercase;
  min-width: 0;
}

.branch-trigger {
  height: 22px;
  max-width: 140px;
  border: 1px solid #2a2a2a;
  border-radius: 6px;
  background: #151515;
  color: #b9b9b9;
  padding: 0 8px;
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}

.branch-trigger:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.git-btn {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.git-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.git-action-btn {
  height: 24px;
  padding: 0 8px;
  border-radius: 6px;
  font-size: 11px;
  line-height: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.git-action-btn:disabled,
.git-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.branch-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  z-index: 60;
}

.branch-modal {
  position: fixed;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  width: min(420px, calc(100vw - 24px));
  max-height: min(520px, calc(100vh - 24px));
  border: 1px solid #333;
  border-radius: 10px;
  background: #141414;
  padding: 10px;
  display: grid;
  grid-template-rows: auto auto minmax(0, 1fr) auto;
  gap: 8px;
  z-index: 61;
}

.branch-modal-title {
  font-size: 13px;
  color: #eaeaea;
}

.branch-search {
  width: 100%;
  border: 1px solid #2a2a2a;
  background: #101010;
  color: #e6e6e6;
  border-radius: 6px;
  padding: 6px 8px;
  font-size: 12px;
}

.branch-list {
  min-height: 120px;
  max-height: 320px;
  overflow: auto;
  border: 1px solid #252525;
  border-radius: 8px;
}

.branch-option {
  width: 100%;
  border: 0;
  border-bottom: 1px solid #1f1f1f;
  background: transparent;
  padding: 8px 10px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  text-align: left;
  font-size: 12px;
  cursor: pointer;
}

.branch-option:hover {
  background: #1d1d1d;
}

.branch-option.active {
  background: #202020;
  color: #c8dcff;
}

.branch-empty {
  margin: 10px;
  font-size: 12px;
  color: #9a9a9a;
}

.branch-modal-actions {
  display: flex;
  justify-content: flex-end;
}

.branch-close-btn {
  height: 26px;
  border-radius: 6px;
  padding: 0 10px;
  cursor: pointer;
}

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
  border-bottom: 1px solid #252525;
}

.git-controls {
  padding: 8px;
  border-bottom: 1px solid #252525;
  display: grid;
  gap: 8px;
}

.git-stats {
  display: flex;
  gap: 10px;
  font-size: 11px;
  color: #a8a8a8;
}

.commit-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 6px;
  align-items: center;
}

.commit-input {
  width: 100%;
  border: 1px solid #2a2a2a;
  background: #151515;
  color: #e9e9e9;
  border-radius: 6px;
  padding: 5px 7px;
  min-width: 0;
}

.commit-btn {
  min-width: 96px;
  height: 26px;
  border-radius: 6px;
  padding: 0 9px;
  gap: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 12px;
}

.commit-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
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

.git-file-list {
  overflow: auto;
  min-height: 0;
}

.file-group + .file-group {
  border-top: 1px solid #262626;
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
  background: #151515;
  border-bottom: 1px solid #242424;
  font-size: 11px;
  color: #a9a9a9;
}

.group-btn {
  height: 20px;
  border-radius: 5px;
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  cursor: pointer;
}

.group-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.file-row {
  width: 100%;
  text-align: left;
  border: 0;
  border-bottom: 1px solid #1f1f1f;
  background: transparent;
  padding: 7px 10px;
  display: grid;
  grid-template-columns: 28px minmax(0, 1fr) auto;
  gap: 8px;
  cursor: pointer;
  align-items: center;
}

.file-row.active {
  background: #1d1d1d;
}

.code {
  color: #9cc5ff;
  font-size: 12px;
}

.path {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
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
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
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
  color: #9a9a9a;
}

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
  color: #9a9a9a;
}

.git-diff {
  min-height: 0;
}
</style>
