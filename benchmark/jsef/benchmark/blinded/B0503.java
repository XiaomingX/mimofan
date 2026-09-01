package blinded;

/*
 * JSEF-Benchmark L5 — Jackson getter 自动调用 gadget（CWE-502 / CWE-915）
 *
 * 验收点：Jackson 在反序列化/序列化（readValue / writeValue）期间会**自动调用**
 * 所有 public getter（getX()）来构造或输出字段。攻击者若能在被反序列化类里
 * 控制一个 getter 的方法体，就可在“框架自动调用路径”上触发危险副作用——
 * 触发点并非调用方显式写出 sink，而是 Jackson 内部反射 invoke。
 *
 * 本样本不 import 任何真实 Jackson 类，仅用教学占位类 UserStub 模拟该语义：
 *   - setXxx() 设置字段；
 *   - getXxx() 被“框架自动调用”，其方法体内嵌模拟危险 sink（语义等价
 *     Runtime.exec，仅 localhost 演示：打印命令并写本地 marker，不真正执行）。
 *
 * 被测工具需识别：“getter 自动调用”即可达危险副作用，而非依赖显式 sink 调用点。
 *
 * 安全底线：仅 localhost 演示语义，不写真实攻击利用脚本，不真正执行命令。
 *
 * 修复要点（对照 JacksonGetterGadgetBy.java）：getter 内不得有任何危险副作用，
 * 仅返回字段；或将副作用逻辑移出 getter 由显式业务方法调用。
 */
public class JacksonGetterGadget {

    



    public static class UserStub {
        private String name;
        private String payload;   // 攻击者可控字段

        public void setName(String name) {
            this.name = name;
        }

        public String getName() {
            return name;
        }

        public void setPayload(String payload) {
            this.payload = payload;
        }

        



        public String getPayload() {
            /*ANCHOR_1*/
            return invokeDangerousSideEffect(payload);   // 框架自动调用此 getter -> 触发副作用
        }

        // 抽象 sink：语义等价 Runtime.getRuntime().exec(cmd)。仅 localhost 演示。
        // 不真正执行命令；打印命令并写本地 marker 表示“已触发”。
        static String invokeDangerousSideEffect(String cmd) {
            System.out.println("[cmd-exec] (localhost demo only) " + cmd);
            System.out.println("[marker] gadget side-effect triggered");
            return cmd;
        }
    }

    



    public static String serialize(UserStub user) {
        // Jackson 自动调用所有 getter；此处模拟该反射调用语义
        return "{\"name\":\"" + user.getName() + "\",\"payload\":\"" + user.getPayload() + "\"}";
    }

    public static void main(String[] args) {
        UserStub user = new UserStub();
        user.setName("victim");
        user.setPayload("touch /tmp/pwned");   // 攻击者通过 JSON 字段注入
        serialize(user);                          // Jackson 自动调用 getPayload() -> 触发副作用
    }
}
