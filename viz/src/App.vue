<template>
    <div class="app-shell">
        <header class="app-topbar">
            <div class="app-brand">
                <span class="app-title">htsim-rs</span>
                <span class="app-subtitle">Visualization & Workload Editor</span>
            </div>
            <div class="app-tabs">
                <button
                    type="button"
                    class="app-tab"
                    :class="{ active: mode === 'playback' }"
                    @click="setMode('playback')"
                >
                    回放
                </button>
                <button
                    type="button"
                    class="app-tab"
                    :class="{ active: mode === 'editor' }"
                    @click="setMode('editor')"
                >
                    Workload 编辑器
                </button>
            </div>
        </header>

        <div v-if="mode === 'playback'" class="app" :class="{ 'sidebar-hidden': !showSidebar, 'event-hidden': !showEvent }">
            <SidebarPanel v-show="showSidebar" class="sidebar" />
            <main>
                <TopologyCard class="card-topology" :meta="player.computed.topologyStatus" @ready="player.actions.setNetCanvas" />
                <TcpCard
                    ref="tcpCardRef"
                    class="card-tcp"
                    @ready="onTcpReady"
                    @modal-ready="player.actions.setTcpModalCanvas"
                    @modal-close="player.actions.onTcpModalClose"
                />
                <TcpDetailsCard class="card-tcp-detail" @ready="player.actions.setTcpDetailCanvas" />
            </main>
            <CurrentEventPanel v-show="showEvent" class="event-panel" />
            <button class="panel-toggle left" type="button" @click="showSidebar = !showSidebar">
                <span class="panel-toggle-icon">{{ showSidebar ? "«" : "»" }}</span>
            </button>
            <button class="panel-toggle right" type="button" @click="showEvent = !showEvent">
                <span class="panel-toggle-icon">{{ showEvent ? "»" : "«" }}</span>
            </button>
        </div>

        <WorkloadEditor v-else />
    </div>
</template>

<script setup>
import { onBeforeUnmount, onMounted, provide, ref } from "vue";
import SidebarPanel from "./components/SidebarPanel.vue";
import TopologyCard from "./components/TopologyCard.vue";
import TcpCard from "./components/TcpCard.vue";
import TcpDetailsCard from "./components/TcpDetailsCard.vue";
import CurrentEventPanel from "./components/CurrentEventPanel.vue";
import WorkloadEditor from "./components/WorkloadEditor.vue";
import { usePlayer } from "./composables/usePlayer";

const player = usePlayer();
const tcpCardRef = ref(null);
const showSidebar = ref(true);
const showEvent = ref(true);
const mode = ref(window.location.hash === "#editor" ? "editor" : "playback");
const handleHash = () => {
    mode.value = window.location.hash === "#editor" ? "editor" : "playback";
};

provide("player", player);

function onTcpReady(canvas) {
    player.actions.setTcpCanvas(canvas);
}

function setMode(next) {
    mode.value = next;
    window.location.hash = next === "editor" ? "editor" : "";
}

onMounted(() => {
    window.addEventListener("hashchange", handleHash);
    if (tcpCardRef.value) {
        player.actions.setTcpCardRef(tcpCardRef.value);
    }
});

onBeforeUnmount(() => {
    window.removeEventListener("hashchange", handleHash);
});
</script>
