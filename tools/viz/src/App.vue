<template>
    <div class="app">
        <SidebarPanel />
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
    </div>
</template>

<script setup>
import { onMounted, provide, ref } from "vue";
import SidebarPanel from "./components/SidebarPanel.vue";
import TopologyCard from "./components/TopologyCard.vue";
import TcpCard from "./components/TcpCard.vue";
import TcpDetailsCard from "./components/TcpDetailsCard.vue";
import { usePlayer } from "./composables/usePlayer";

const player = usePlayer();
const tcpCardRef = ref(null);

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
