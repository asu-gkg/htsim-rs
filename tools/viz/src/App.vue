<template>
    <div class="app">
        <SidebarPanel />
        <main>
            <TopologyCard class="card-topology" :meta="player.computed.topologyStatus" @ready="player.actions.setNetCanvas" />
            <TcpCard class="card-tcp" @ready="player.actions.setTcpCanvas" />
            <TcpDetailsCard class="card-tcp-detail" @ready="player.actions.setTcpDetailCanvas" />
        </main>
    </div>
</template>

<script setup>
import { provide } from "vue";
import SidebarPanel from "./components/SidebarPanel.vue";
import TopologyCard from "./components/TopologyCard.vue";
import TcpCard from "./components/TcpCard.vue";
import TcpDetailsCard from "./components/TcpDetailsCard.vue";
import { usePlayer } from "./composables/usePlayer";

const player = usePlayer();

provide("player", player);
</script>
