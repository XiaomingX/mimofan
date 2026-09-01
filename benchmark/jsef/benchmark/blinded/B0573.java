package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RestController;


























@RestController
public class EntryDeser {

    private final Gateway gateway = new Gateway();
    private final Deserializer deserializer = new Deserializer();
    private final Repository repo = new Repository();

    @PostMapping("/benchmark/longrange/deser/unby")
    public String handle(@RequestBody byte[] requestBody,
                         @RequestHeader("X-Topic") String topic) {
        // 入口：不可信请求体 + 路由头进入链路
        Gateway.GatewayEnvelope env = gateway.forward(requestBody, topic); // 传递点 2（见 Gateway.java:46）
        Object obj = deserializer.deserialize(env);                        // 传递点 3-5（见 Deserializer.java:46,49）

        /*ANCHOR_1*/
        return repo.save(obj); // 持久化 -> 触发反序列化对象危险 getter（sink）
    }

    
    static class Repository {
        String save(Object obj) {
            // 语义等价：jpaRepository.save(obj) / cache.put(obj)
            // 触发 obj 的危险 getter（如 Jackson 多态指定类的 @JsonCreator）
            System.out.println("[persist] " + obj);
            return "saved:" + obj;
        }
    }
}
