package blinded;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

import javax.websocket.OnClose;
import javax.websocket.OnMessage;
import javax.websocket.OnOpen;
import javax.websocket.ServerEndpoint;
import javax.websocket.Session;










@ServerEndpoint("/chat")
public class WebSocketNoAuthzBy {

    
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
        
        /*ANCHOR_1*/
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
        System.out.println("[ws-transfer-by] " + toUserId + " +" + amount);
    }
}
