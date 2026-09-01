package blinded;












public class Gateway {

    
    public static class GatewayEnvelope {
        private final byte[] rawPayload;
        private final String topic;
        private final String tenant;

        public GatewayEnvelope(byte[] rawPayload, String topic, String tenant) {
            // 污点流入点：rawPayload 直接来自不可信请求体，原样保留
            this.rawPayload = rawPayload;
            this.topic = topic;
            this.tenant = tenant;
        }

        public byte[] getRawPayload() {
            return rawPayload; // 污点从这里流出到反序列化模块
        }

        public String getTopic() {
            return topic;
        }

        public String getTenant() {
            return tenant;
        }
    }

    






    public GatewayEnvelope forward(byte[] requestBody, String topic) {
        // 中间传递点 1：不可信字节流原样进入信封
        String tenant = resolveTenant(requestBody);        // 中间传递点 2（无害路由逻辑）
        return new GatewayEnvelope(requestBody, topic, tenant);
    }

    
    static String resolveTenant(byte[] body) {
        // 语义等价：从请求头/Token 解析租户
        return "tenant-default";
    }
}
