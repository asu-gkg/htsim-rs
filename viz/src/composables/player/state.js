import { reactive } from "vue";
import { defaultLinks, defaultNodes } from "../../utils/layout";

export function createPlayerState() {
    return reactive({
        events: [],
        filtered: [],
        meta: null,
        layoutChoice: "auto",
        layoutDetected: "-",
        filterFlow: "",
        filterPkt: "",
        connPick: "auto",
        connOptions: [],
        speed: 1,
        targetWallSec: 20,
        playing: false,
        t0: 0,
        t1: 0,
        curTime: 0,
        cursor: 0,
        lastWall: 0,
        slider: 0,
        inflight: new Map(),
        nodeHighlight: new Map(),
        dropMarks: [],
        lastEventsText: [],
        focusEvents: [],
        focusWindowNs: 500_000,
        nodes: defaultNodes.map((n) => ({ ...n })),
        nodeById: new Map(),
        drawLinks: defaultLinks.slice(),
        nodeScale: 1,
        nodeStats: new Map(),
        linkStats: new Map(),
        tcpStats: { send_data: 0, send_ack: 0, recv_ack: 0, rto: 0, retrans: 0 },
        tcpSeries: new Map(),
        curText: "（空）",
        statsText: "（空）",
        maxLinkBandwidth: 0, // 用于判断瓶颈链路
        eventTypeFilter: {},
    });
}
