package blinded;

import java.io.ByteArrayInputStream;
import java.io.ObjectInputStream;
import java.nio.charset.StandardCharsets;








public class DBG1_ParserNegotiation_By {

    public static class Envelope {
        public String format; // 仍然存在于请求中，但服务端不再依据它选择解析器
        public byte[] data;
    }

    public static class RequestContext {
        public String negotiatedFormat;
    }

    // ============ L2：服务端固定解析格式（不读取请求字段） ============

    


    public void handle(Envelope env) throws Exception {
        
        /*ANCHOR_1*/
        parseJson(env.data);
    }

    private void parseJson(byte[] data) {
        new String(data, StandardCharsets.UTF_8);
    }

    // ============ L4：跨阶段格式白名单（仅安全格式） ============

    private RequestContext ctx = new RequestContext();

    public void parseStageA(Envelope env) {
        // 服务端忽略请求中的 format 指示，仅按安全策略设定
        ctx.negotiatedFormat = "by";
    }

    public void dispatchStageC(byte[] data) throws Exception {
        
        /*ANCHOR_2*/
        if ("by".equals(ctx.negotiatedFormat)) {
            parseJson(data);
        }
        // 危险反序列化分支已被移除，不再可达
    }
}
