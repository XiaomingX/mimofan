package blinded;

import java.io.ByteArrayInputStream;
import java.io.ObjectInputStream;
import java.nio.charset.StandardCharsets;









public class DBG1_ParserNegotiation {

    
    public static class Envelope {
        public String format; // 客户端可控制的格式指示
        public byte[] data;
    }

    // ============ L2：单方法内格式切换 ============

    



    public void handle(Envelope env) throws Exception {
        if (env.format.equals("by")) {
            parseJson(env.data);
        } else {
            
            /*ANCHOR_1*/
            ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(env.data));
            ois.readObject();
        }
    }

    private void parseJson(byte[] data) {
        // localhost-demo：安全的 JSON 解析占位
        new String(data, StandardCharsets.UTF_8);
    }

    // ============ L4：跨阶段格式切换（单文件内模拟跨阶段） ============

    





    
    public static class RequestContext {
        public String negotiatedFormat;
    }

    private RequestContext ctx = new RequestContext();

    
    public void parseStageA(Envelope env) {
        
        // 行A：format 指示解析点
        ctx.negotiatedFormat = env.format;
    }

    
    public void dispatchStageC(byte[] data) throws Exception {
        String format = ctx.negotiatedFormat;
        if (!"by".equals(format)) {
            
            /*ANCHOR_2*/
            ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(data));
            ois.readObject();
        } else {
            parseJson(data);
        }
    }
}
