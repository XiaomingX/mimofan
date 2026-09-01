package com.jsef.benchmark.sec.websocketauthz;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

import javax.websocket.OnClose;
import javax.websocket.OnMessage;
import javax.websocket.OnOpen;
import javax.websocket.ServerEndpoint;
import javax.websocket.Session;

/**
 * JSEF-Benchmark L3 — WebSocket 缺鉴权修复（CWE-862 SAFE）
 *
 * 修复：onOpen 校验握手身份（principal 为空直接关闭会话）；
 * 消息处理每次用 session.getUserPrincipal() / 服务端会话身份映射复核，
 * 绝不信任消息内携带的 userId。
 *
 * CWE-862 (Missing Authorization)。
 */
@ServerEndpoint("/chat")
public class WebSocketNoAuthzSafe {

    /** 会话 id -> 服务端身份（onOpen 校验后写入） */
    private final Map<String, String> sessionIdentity = new ConcurrentHashMap<>();

    @OnOpen
    public void onOpen(Session session) {
        // onOpen 校验握手身份：principal 为空则拒绝连接
        if (session.getUserPrincipal() == null) {
            abort(session);
            return;
        }
        sessionIdentity.put(session.getId(), session.getUserPrincipal().getName());
    }

    @OnMessage
    public void onMessage(String raw, Session session) {
        // 用服务端会话身份复核，不信任消息内容
        String serverUserId = sessionIdentity.get(session.getId());
        int amount = parseAmount(raw);
        // [VULN] 安全：目标账号取自服务端会话身份，消息内 userId 被忽略
        // [CHECKPOINT id=JSEF-WS-001S cwe=862 level=L3 source=websocket message sink=session.getUserPrincipal() re-check expect=SAFE]
        transfer(serverUserId, amount, session);
    }

    @OnClose
    public void onClose(Session session) {
        sessionIdentity.remove(session.getId());
    }

    // ---- stubs（仅演示语义，不落真实实现）----

    static void abort(Session session) {
        System.out.println("[ws] abort unauthenticated session");
    }

    static int parseAmount(String raw) {
        return 100;
    }

    static void transfer(String toUserId, int amount, Session session) {
        System.out.println("[ws-transfer-safe] " + toUserId + " +" + amount);
    }
}
