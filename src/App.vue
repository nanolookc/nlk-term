<script setup lang="ts">
import {
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuPortal,
  ContextMenuRoot,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "reka-ui";
import { Minus, PanelRightClose, PanelRightOpen, Pencil, Plus, Square, TerminalSquare, X } from "lucide-vue-next";
import GitPanel from "./components/GitPanel.vue";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import TerminalPane from "./components/TerminalPane.vue";

type Project = {
  id: string;
  name: string;
};

type TerminalTab = {
  id: string;
  projectId: string;
  title: string;
};

type PersistedState = {
  projects: Project[];
  tabs: TerminalTab[];
  activeProjectId: string | null;
  activeTabId: string | null;
};

type RuntimeSession = {
  shell?: string;
  title?: string;
};

const STORAGE_KEY = "workspace-state-v5";
const appWindow = getCurrentWindow();

const creatingProject = ref(false);
const newProjectName = ref("");
const editingProjectId = ref<string | null>(null);
const editingProjectName = ref("");
const editingTabId = ref<string | null>(null);
const editingTabName = ref("");
const newProjectInput = ref<HTMLInputElement | null>(null);
const runtimeSessions = ref<Record<string, RuntimeSession>>({});
const gitPanelOpen = ref(true);
const activeTerminalCwd = ref<string | null>(null);
let cwdPollTimer: number | null = null;

const projectInputRefs = new Map<string, HTMLInputElement>();
const tabInputRefs = new Map<string, HTMLInputElement>();

function uid(prefix: string) {
  return `${prefix}-${Math.random().toString(36).slice(2, 9)}`;
}

function normalizeShell(shell: string) {
  const parts = shell.split(/[\\/]/);
  return parts[parts.length - 1] || shell;
}

function defaultState(): PersistedState {
  const projectId = uid("project");
  const tabId = uid("tab");

  return {
    projects: [{ id: projectId, name: "project" }],
    tabs: [{ id: tabId, projectId, title: "session-1" }],
    activeProjectId: projectId,
    activeTabId: tabId,
  };
}

function loadState(): PersistedState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaultState();

    const parsed = JSON.parse(raw) as PersistedState;
    if (!parsed.projects?.length) return defaultState();

    const activeProjectId = parsed.activeProjectId ?? parsed.projects[0]?.id ?? null;
    const tabs = parsed.tabs ?? [];
    const firstTab = tabs.find((tab) => tab.projectId === activeProjectId) ?? null;

    return {
      projects: parsed.projects,
      tabs,
      activeProjectId,
      activeTabId: parsed.activeTabId ?? firstTab?.id ?? null,
    };
  } catch {
    return defaultState();
  }
}

const state = ref<PersistedState>(loadState());

watch(
  state,
  (next) => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  },
  { deep: true },
);

const activeProject = computed(() =>
  state.value.projects.find((project) => project.id === state.value.activeProjectId) ?? null,
);

const projectTabs = computed(() => {
  if (!state.value.activeProjectId) return [];
  return state.value.tabs.filter((tab) => tab.projectId === state.value.activeProjectId);
});

const activeTab = computed(() =>
  state.value.tabs.find((tab) => tab.id === state.value.activeTabId) ?? projectTabs.value[0] ?? null,
);

function tabLabel(tab: TerminalTab) {
  const runtime = runtimeSessions.value[tab.id];

  if (runtime?.title?.trim()) {
    return runtime.title.trim();
  }

  if (runtime?.shell) {
    return `${runtime.shell} session`;
  }

  return tab.title;
}

function setProjectInputRef(projectId: string, element: unknown) {
  if (element instanceof HTMLInputElement) {
    projectInputRefs.set(projectId, element);
    return;
  }

  projectInputRefs.delete(projectId);
}

function setTabInputRef(tabId: string, element: unknown) {
  if (element instanceof HTMLInputElement) {
    tabInputRefs.set(tabId, element);
    return;
  }

  tabInputRefs.delete(tabId);
}

function setActiveProject(projectId: string) {
  state.value.activeProjectId = projectId;
  const nextTab = state.value.tabs.find((tab) => tab.projectId === projectId);
  state.value.activeTabId = nextTab?.id ?? null;
}

async function startCreateProject() {
  if (creatingProject.value) return;

  creatingProject.value = true;
  newProjectName.value = `project-${state.value.projects.length + 1}`;

  await nextTick();
  newProjectInput.value?.focus();
  newProjectInput.value?.select();
}

function cancelCreateProject() {
  creatingProject.value = false;
  newProjectName.value = "";
}

function commitCreateProject() {
  const name = newProjectName.value.trim();
  if (!name) {
    cancelCreateProject();
    return;
  }

  const projectId = uid("project");
  const tabId = uid("tab");

  state.value.projects.push({ id: projectId, name });
  state.value.tabs.push({ id: tabId, projectId, title: "session-1" });
  state.value.activeProjectId = projectId;
  state.value.activeTabId = tabId;

  cancelCreateProject();
}

async function startRenameProject(projectId: string) {
  const project = state.value.projects.find((item) => item.id === projectId);
  if (!project) return;

  editingProjectId.value = projectId;
  editingProjectName.value = project.name;

  await nextTick();
  const input = projectInputRefs.get(projectId);
  input?.focus();
  input?.select();
}

function cancelRenameProject() {
  editingProjectId.value = null;
  editingProjectName.value = "";
}

function commitRenameProject() {
  if (!editingProjectId.value) return;

  const next = editingProjectName.value.trim();
  const project = state.value.projects.find((item) => item.id === editingProjectId.value);

  if (next && project) {
    project.name = next;
  }

  cancelRenameProject();
}

function deleteProject(projectId: string) {
  if (state.value.projects.length <= 1) return;

  const remainingProjects = state.value.projects.filter((project) => project.id !== projectId);
  const removedTabs = state.value.tabs.filter((tab) => tab.projectId === projectId).map((tab) => tab.id);
  const remainingTabs = state.value.tabs.filter((tab) => tab.projectId !== projectId);

  const nextRuntime = { ...runtimeSessions.value };
  for (const tabId of removedTabs) {
    delete nextRuntime[tabId];
  }

  state.value.projects = remainingProjects;
  state.value.tabs = remainingTabs;
  runtimeSessions.value = nextRuntime;

  const nextProjectId =
    state.value.activeProjectId === projectId
      ? remainingProjects[0]?.id ?? null
      : state.value.activeProjectId;

  state.value.activeProjectId = nextProjectId;

  const nextTab = state.value.tabs.find((tab) => tab.projectId === nextProjectId);
  state.value.activeTabId = nextTab?.id ?? null;
}

function createTab() {
  if (!state.value.activeProjectId) return;

  const count = projectTabs.value.length + 1;
  const tab: TerminalTab = {
    id: uid("tab"),
    projectId: state.value.activeProjectId,
    title: `session-${count}`,
  };

  state.value.tabs.push(tab);
  state.value.activeTabId = tab.id;
}

async function startRenameTab(tabId: string) {
  const tab = state.value.tabs.find((item) => item.id === tabId);
  if (!tab) return;

  editingTabId.value = tabId;
  editingTabName.value = tab.title;

  await nextTick();
  const input = tabInputRefs.get(tabId);
  input?.focus();
  input?.select();
}

function cancelRenameTab() {
  editingTabId.value = null;
  editingTabName.value = "";
}

function commitRenameTab() {
  if (!editingTabId.value) return;

  const next = editingTabName.value.trim();
  const tab = state.value.tabs.find((item) => item.id === editingTabId.value);

  if (next && tab) {
    tab.title = next;
  }

  cancelRenameTab();
}

function closeTab(tabId: string) {
  const tab = state.value.tabs.find((item) => item.id === tabId);
  if (!tab) return;

  state.value.tabs = state.value.tabs.filter((item) => item.id !== tabId);

  const nextRuntime = { ...runtimeSessions.value };
  delete nextRuntime[tabId];
  runtimeSessions.value = nextRuntime;

  if (!state.value.tabs.some((item) => item.projectId === tab.projectId)) {
    const replacement = {
      id: uid("tab"),
      projectId: tab.projectId,
      title: "session-1",
    };

    state.value.tabs.push(replacement);
  }

  if (state.value.activeTabId === tabId) {
    const next = state.value.tabs.find((item) => item.projectId === tab.projectId);
    state.value.activeTabId = next?.id ?? null;
  }
}

function onSessionMeta(payload: { tabId: string; shell: string }) {
  runtimeSessions.value = {
    ...runtimeSessions.value,
    [payload.tabId]: {
      ...runtimeSessions.value[payload.tabId],
      shell: normalizeShell(payload.shell),
    },
  };
}

function onSessionTitle(payload: { tabId: string; title: string }) {
  runtimeSessions.value = {
    ...runtimeSessions.value,
    [payload.tabId]: {
      ...runtimeSessions.value[payload.tabId],
      title: payload.title,
    },
  };
}

function minimizeWindow() {
  appWindow.minimize().catch(() => undefined);
}

function toggleMaximizeWindow() {
  appWindow.toggleMaximize().catch(() => undefined);
}

function closeWindow() {
  appWindow.close().catch(() => undefined);
}

function toggleGitPanel() {
  gitPanelOpen.value = !gitPanelOpen.value;
}

async function syncActiveTerminalCwd() {
  const tabId = activeTab.value?.id;
  if (!tabId) {
    activeTerminalCwd.value = null;
    return;
  }

  try {
    const cwd = await invoke<string | null>("terminal_cwd", { tabId });
    activeTerminalCwd.value = cwd;
  } catch {
    activeTerminalCwd.value = null;
  }
}

function startWindowDragFromHeader(event: MouseEvent) {
  if (event.button !== 0) return;
  const target = event.target as HTMLElement | null;
  if (
    target?.closest(
      "button, input, textarea, [contenteditable='true'], .window-tab, .ctx-menu, [role='menuitem']",
    )
  ) {
    return;
  }

  appWindow.startDragging().catch(() => undefined);
}

onMounted(() => {
  syncActiveTerminalCwd().catch(() => undefined);
  cwdPollTimer = window.setInterval(() => {
    syncActiveTerminalCwd().catch(() => undefined);
  }, 1200);
});

onBeforeUnmount(() => {
  if (cwdPollTimer !== null) {
    window.clearInterval(cwdPollTimer);
  }
});

watch(
  () => activeTab.value?.id,
  () => {
    syncActiveTerminalCwd().catch(() => undefined);
  },
);
</script>

<template>
  <div class="window-frame">
    <div class="layout">
      <aside class="sidebar">
        <div class="sidebar-head">
          <span>projects</span>
          <button class="add-btn" @click="startCreateProject">
            <Plus :size="14" />
          </button>
        </div>

        <div class="project-list">
          <ContextMenuRoot v-for="project in state.projects" :key="project.id">
            <ContextMenuTrigger as-child>
              <div class="project-main" :class="{ active: project.id === state.activeProjectId }" @click="setActiveProject(project.id)">
                <input
                  v-if="editingProjectId === project.id"
                  :ref="(el) => setProjectInputRef(project.id, el)"
                  v-model="editingProjectName"
                  class="inline-input"
                  @click.stop
                  @keydown.enter.prevent="commitRenameProject"
                  @keydown.esc.prevent="cancelRenameProject"
                  @blur="commitRenameProject"
                />
                <span v-else class="project-name">{{ project.name }}</span>

                <button class="project-edit" @click.stop="startRenameProject(project.id)">
                  <Pencil :size="12" />
                </button>
              </div>
            </ContextMenuTrigger>
            <ContextMenuPortal>
              <ContextMenuContent class="ctx-menu" :side-offset="6">
                <ContextMenuItem class="ctx-item" @select="startRenameProject(project.id)">
                  Rename project
                </ContextMenuItem>
                <ContextMenuSeparator class="ctx-sep" />
                <ContextMenuItem class="ctx-item danger" @select="deleteProject(project.id)">
                  Delete project
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenuPortal>
          </ContextMenuRoot>

          <form v-if="creatingProject" class="project-form" @submit.prevent="commitCreateProject">
            <input
              ref="newProjectInput"
              v-model="newProjectName"
              class="inline-input"
              @keydown.esc.prevent="cancelCreateProject"
              @blur="commitCreateProject"
            />
          </form>
        </div>
      </aside>

      <main class="main">
        <header class="titlebar" @mousedown="startWindowDragFromHeader">
          <div class="titlebar-left">
            <div class="tabs-scroll">
              <ContextMenuRoot v-for="tab in projectTabs" :key="tab.id">
                <ContextMenuTrigger as-child>
                  <div class="window-tab" :class="{ active: tab.id === activeTab?.id }">
                    <button class="window-main" @click="state.activeTabId = tab.id">
                      <TerminalSquare :size="14" class="tab-icon" />
                      <input
                        v-if="editingTabId === tab.id"
                        :ref="(el) => setTabInputRef(tab.id, el)"
                        v-model="editingTabName"
                        class="inline-input tab-input"
                        @click.stop
                        @keydown.enter.prevent="commitRenameTab"
                        @keydown.esc.prevent="cancelRenameTab"
                        @blur="commitRenameTab"
                      />
                      <span v-else @dblclick.stop="startRenameTab(tab.id)">{{ tabLabel(tab) }}</span>
                    </button>
                    <button class="icon-btn close" type="button" title="Close tab" aria-label="Close tab" @click="closeTab(tab.id)">
                      <X :size="13" />
                    </button>
                  </div>
                </ContextMenuTrigger>
                <ContextMenuPortal>
                  <ContextMenuContent class="ctx-menu" :side-offset="6">
                    <ContextMenuItem class="ctx-item" @select="startRenameTab(tab.id)">
                      Rename tab
                    </ContextMenuItem>
                    <ContextMenuSeparator class="ctx-sep" />
                    <ContextMenuItem class="ctx-item danger" @select="closeTab(tab.id)">
                      Close tab
                    </ContextMenuItem>
                  </ContextMenuContent>
                </ContextMenuPortal>
              </ContextMenuRoot>

              <button class="add-btn tab-add-btn" type="button" title="New tab" aria-label="New tab" @click="createTab">
                <Plus :size="14" />
              </button>
            </div>
          </div>

          <div class="titlebar-drag" data-tauri-drag-region>
            <div class="titlebar-title">nlk-term</div>
          </div>

          <div class="titlebar-controls">
            <button
              class="titlebar-btn"
              type="button"
              :title="gitPanelOpen ? 'Hide git panel' : 'Show git panel'"
              :aria-label="gitPanelOpen ? 'Hide git panel' : 'Show git panel'"
              @click="toggleGitPanel"
            >
              <PanelRightClose v-if="gitPanelOpen" :size="13" />
              <PanelRightOpen v-else :size="13" />
            </button>
            <button class="titlebar-btn" type="button" title="Minimize window" aria-label="Minimize window" @click="minimizeWindow">
              <Minus :size="13" />
            </button>
            <button class="titlebar-btn" type="button" title="Toggle maximize window" aria-label="Toggle maximize window" @click="toggleMaximizeWindow">
              <Square :size="12" />
            </button>
            <button class="titlebar-btn close" type="button" title="Close window" aria-label="Close window" @click="closeWindow">
              <X :size="13" />
            </button>
          </div>
        </header>

        <section class="work-area">
          <div class="terminal-area">
            <TerminalPane
              v-for="tab in state.tabs"
              :key="tab.id"
              :tab-id="tab.id"
              :active="tab.id === activeTab?.id && tab.projectId === activeProject?.id"
              @session-meta="onSessionMeta"
              @session-title="onSessionTitle"
            />
          </div>
          <GitPanel :open="gitPanelOpen" :repo-path="activeTerminalCwd" />
        </section>
      </main>
    </div>
  </div>
</template>
