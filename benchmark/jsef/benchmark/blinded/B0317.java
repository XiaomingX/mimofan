package blinded;

import javax.script.ScriptEngine;
import javax.script.ScriptEngineManager;
import java.lang.reflect.Field;
import java.util.Arrays;
import java.util.List;
import java.util.Map;















public class TCM4_PrivateFieldBind_By {

    // L2：字段白名单——只允许安全字段
    private static final List<String> ALLOWED_FIELDS = Arrays.asList("label", "timeout");

    public static class CommandTarget {
        @SuppressWarnings("unused")
        private String command; // 危险字段：final/只读，外部不可写
        @SuppressWarnings("unused")
        private String label;

        public void bindField(String name, String value) throws Exception {
            if (!ALLOWED_FIELDS.contains(name)) {
                throw new IllegalArgumentException("field not in allowlist: " + name);
            }
            Field f = CommandTarget.class.getDeclaredField(name);
            f.setAccessible(true);
            f.set(this, value);
        }

        public void execute() throws Exception {
            // 即便执行，也只针对白名单字段，command 不可被外部设置
            System.out.println("execute (by, no private command bound)");
        }
    }

    public static class EngineTarget {
        // 引擎由服务端固定构造，不暴露可写私有字段
        private final ScriptEngine engine;

        public EngineTarget() {
            this.engine = new ScriptEngineManager().getEngineByName("js");
        }

        public Object evalLazy() throws Exception {
            // 引擎固定，不依赖外部注入
            return engine.eval("localhost-demo"); // 仅占位，不连真实远端
        }
    }

    


    public void bind(Map<String, String> fields) throws Exception {
        CommandTarget target = new CommandTarget();
        for (Map.Entry<String, String> e : fields.entrySet()) {
            
            /*ANCHOR_1*/
            target.bindField(e.getKey(), e.getValue());
        }
        target.execute();
    }

    


    public void bindEngine(Map<String, Object> fields) throws Exception {
        EngineTarget target = new EngineTarget();
        for (Map.Entry<String, Object> e : fields.entrySet()) {
            
            /*ANCHOR_2*/
            // 服务器固定引擎，忽略外部任何试图写 engine 的字段
            if ("engine".equals(e.getKey())) {
                throw new IllegalArgumentException("dynamic engine bind rejected");
            }
        }
        target.evalLazy();
    }
}
