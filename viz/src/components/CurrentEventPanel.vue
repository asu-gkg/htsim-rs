<template>
    <aside class="event-panel">
        <div class="event-header">
            <div class="event-title">当前事件</div>
            <div class="event-window">{{ focusView.windowLabel }}</div>
        </div>

        <div class="event-summary">
            <div class="event-summary-title">发生了什么</div>
            <div class="event-summary-total">本时间片共 {{ focusView.total }} 条事件</div>
            <ul class="event-summary-list">
                <li v-for="(item, idx) in focusView.highlights" :key="`highlight-${idx}`">
                    {{ item }}
                </li>
            </ul>
        </div>

        <div v-if="focusView.empty" class="event-empty">
            还没有可以解读的事件，尝试播放或单步前进。
        </div>

        <div v-else class="event-groups">
            <div v-for="group in focusView.groups" :key="group.id" class="event-group">
                <div class="event-group-title">
                    <span>{{ group.title }}</span>
                    <span class="event-count">{{ group.count }}</span>
                </div>
                <div class="event-items">
                    <div
                        v-for="(item, idx) in group.items"
                        :key="`${group.id}-${idx}`"
                        class="event-item"
                        :class="[item.severity, item.category, { active: item.isPrimary }]"
                    >
                        <div class="event-time">{{ item.time }}</div>
                        <div class="event-main">
                            <div class="event-item-title">
                                <span>{{ item.title }}</span>
                                <span v-if="item.reasonLabel" class="event-item-tag">{{ item.reasonLabel }}</span>
                            </div>
                            <div class="event-item-detail">{{ item.detail }}</div>
                            <div v-if="item.note" class="event-item-note">{{ item.note }}</div>
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <div class="event-filter">
            <div class="event-summary-title">事件过滤</div>
            <div class="event-filter-note">点击标签可隐藏该类型事件（播放/单步/跳转会跳过，不影响原始数据）。</div>
            <div v-for="group in filterGroups" :key="group.id" class="event-filter-group">
                <div class="event-filter-group-title">{{ group.title }}</div>
                <div class="event-filter-list">
                    <button
                        v-for="item in group.items"
                        :key="item.kind"
                        type="button"
                        class="event-filter-chip"
                        :class="[
                            { off: eventTypeFilters[item.kind] },
                            { sub: item.group === 'cwnd_reason' },
                            { pass: item.group === 'cwnd_reason' && cwndDisabled && !eventTypeFilters[item.kind] },
                            { blocked: item.group === 'cwnd_reason' && cwndDisabled && eventTypeFilters[item.kind] },
                        ]"
                        @click="actions.toggleEventKind(item.kind)"
                    >
                        {{ filterLabel(item) }}
                    </button>
                </div>
                <div
                    v-if="group.id === 'cwnd_reason' && cwndDisabled"
                    class="event-filter-note event-filter-note-inline"
                >
                    总开关关闭时，蓝色为放行，灰色为隐藏。
                </div>
            </div>
        </div>
    </aside>
</template>

<script setup>
import { computed, inject } from "vue";

const player = inject("player");
if (!player) {
    throw new Error("player store not provided");
}

const focusView = player.computed.focusView;
const eventTypeCatalog = player.computed.eventTypeCatalog;
const eventTypeFilters = player.computed.eventTypeFilters;
const actions = player.actions;

const filterGroups = computed(() => {
    const catalog = Array.isArray(eventTypeCatalog) ? eventTypeCatalog : eventTypeCatalog.value || [];
    const order = [
        { id: "base", title: "基础事件" },
        { id: "cwnd", title: "窗口调整（总）" },
        { id: "cwnd_reason", title: "窗口调整原因" },
        { id: "compute", title: "GPU 计算" },
    ];
    const groups = new Map(order.map((g) => [g.id, { ...g, items: [] }]));
    for (const item of catalog) {
        const id = item.group || "base";
        if (!groups.has(id)) groups.set(id, { id, title: id, items: [] });
        groups.get(id).items.push(item);
    }
    const ordered = order.map((g) => groups.get(g.id)).filter((g) => g && g.items.length);
    for (const item of groups.values()) {
        if (order.some((g) => g.id === item.id)) continue;
        if (item.items.length) ordered.push(item);
    }
    return ordered;
});

const cwndDisabled = computed(() => {
    const filters = eventTypeFilters.value || eventTypeFilters || {};
    return Boolean(filters.dctcp_cwnd);
});

function filterLabel(item) {
    if (item.group === "cwnd_reason") {
        return String(item.label || "").replace("窗口调整/", "") || "未知原因";
    }
    return item.label || "未知事件";
}
</script>
