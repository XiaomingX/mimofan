package blinded;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;



















public class Deserializer {

    // 真实库常见危险配置：开启默认多态类型（等价于 enableDefaultTyping）
    private final ObjectMapper mapper = new ObjectMapper();

    





    public Object deserialize(Gateway.GatewayEnvelope env) {
        byte[] payload = env.getRawPayload();                 // 中间传递点 3（污点进入反序列化）
        try {
            // 中间传递点 4：不可信字节流被读成 JsonNode 树
            JsonNode node = mapper.readTree(payload);         // 语义等价：ObjectMapper.readTree(payload)
            // 中间传递点 5：多态 materialize 成具体类（危险：类型由不可信数据指定）
            // 语义等价：mapper.readValue(payload, Object.class) 且开启多态类型
            Object materialized = mapper.treeToValue(node, Object.class);
            return materialized;
        } catch (Exception e) {
            // 退化到原生反序列化（同样危险）：ObjectInputStream.readObject()
            // 语义等价：ois.readObject()
            return fallbackRaw(payload);
        }
    }

    
    private Object fallbackRaw(byte[] payload) {
        // 语义等价：new ObjectInputStream(...).readObject()
        System.out.println("[deser-fallback] " + payload.length + " bytes");
        return payload;
    }
}
