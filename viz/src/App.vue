<template>
    <div class="app-shell">
        <header class="app-topbar">
            <div class="app-brand">
                <span class="app-title">htsim-rs</span>
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
                    :class="{ active: mode === 'sim' }"
                    @click="setMode('sim')"
                >
                    仿真运行
                </button>
                <button
                    type="button"
                    class="app-tab"
                    :class="{ active: mode === 'sim_multi' }"
                    @click="setMode('sim_multi')"
                >
                    仿真运行（多租户）
                </button>
                <button
                    type="button"
                    class="app-tab"
                    :class="{ active: mode === 'neusight' }"
                    @click="setMode('neusight')"
                >
                    NeuSight workload生成器
                </button>
                <button
                    type="button"
                    class="app-tab"
                    :class="{ active: mode === 'hook' }"
                    @click="setMode('hook')"
                >
                    真实workload生成器
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

        <WorkloadSim v-else-if="mode === 'sim'" />
        <WorkloadsSim v-else-if="mode === 'sim_multi'" />
        <WorkloadEditor v-else-if="mode === 'neusight'" generator="prediction" />
        <WorkloadEditor v-else generator="hook" />
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
import WorkloadSim from "./components/WorkloadSim.vue";
import WorkloadsSim from "./components/WorkloadsSim.vue";
import { usePlayer } from "./composables/usePlayer";

const player = usePlayer();
const tcpCardRef = ref(null);
const showSidebar = ref(true);
const showEvent = ref(true);
const mode = ref(
    window.location.hash === "#editor"
        ? "neusight"
        : window.location.hash === "#sim"
        ? "sim"
        : window.location.hash === "#sim-multi"
        ? "sim_multi"
        : window.location.hash === "#hook"
        ? "hook"
        : "playback"
);
const handleHash = () => {
    if (window.location.hash === "#editor") {
        mode.value = "neusight";
        return;
    }
    if (window.location.hash === "#sim") {
        mode.value = "sim";
        return;
    }
    if (window.location.hash === "#sim-multi") {
        mode.value = "sim_multi";
        return;
    }
    if (window.location.hash === "#hook") {
        mode.value = "hook";
        return;
    }
    mode.value = "playback";
};

provide("player", player);

function onTcpReady(canvas) {
    player.actions.setTcpCanvas(canvas);
}

function setMode(next) {
    mode.value = next;
    if (next === "neusight") {
        window.location.hash = "editor";
        return;
    }
    if (next === "sim") {
        window.location.hash = "sim";
        return;
    }
    if (next === "sim_multi") {
        window.location.hash = "sim-multi";
        return;
    }
    if (next === "hook") {
        window.location.hash = "hook";
        return;
    }
    window.location.hash = "";
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
