<template>
    <div class="app" :class="{ 'sidebar-hidden': !showSidebar, 'event-hidden': !showEvent }">
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
</template>

<script setup>
import { onMounted, provide, ref } from "vue";
import SidebarPanel from "./components/SidebarPanel.vue";
import TopologyCard from "./components/TopologyCard.vue";
import TcpCard from "./components/TcpCard.vue";
import TcpDetailsCard from "./components/TcpDetailsCard.vue";
import CurrentEventPanel from "./components/CurrentEventPanel.vue";
import { usePlayer } from "./composables/usePlayer";

const player = usePlayer();
const tcpCardRef = ref(null);
const showSidebar = ref(true);
const showEvent = ref(true);

provide("player", player);

function onTcpReady(canvas) {
    player.actions.setTcpCanvas(canvas);
}

onMounted(() => {
    if (tcpCardRef.value) {
        player.actions.setTcpCardRef(tcpCardRef.value);
    }
});
</script>
