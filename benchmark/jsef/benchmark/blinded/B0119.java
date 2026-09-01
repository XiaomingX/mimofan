package blinded;

/*
 * JSEF-Benchmark L5 — Jackson getter 自动调用 gadget 安全对照（CWE-502 / CWE-915）
 *
 * 修复：getter 内无任何危险副作用，仅返回字段值。副作用逻辑不放在 getter 中，
 * 因此 Jackson 自动调用 getter 时不会触发任何危险操作。
 *
 * BX 侧按实现判安全：getter 仅返回字段，无 sink 语义。
 */
public class JacksonGetterGadgetBy {

    


    public static class UserStub {
        private String name;
        private String payload;

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
            return payload;
        }
    }

    


    public static String serialize(UserStub user) {
        return "{\"name\":\"" + user.getName() + "\",\"payload\":\"" + user.getPayload() + "\"}";
    }

    public static void main(String[] args) {
        UserStub user = new UserStub();
        user.setName("victim");
        user.setPayload("touch /tmp/pwned");
        serialize(user);
    }
}
