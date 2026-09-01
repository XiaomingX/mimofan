package blinded;

import javax.script.ScriptEngine;
import javax.script.ScriptEngineManager;
import java.lang.reflect.Field;
import java.util.Map;
















public class TCM4_PrivateFieldBind {

    


    public static class CommandTarget {
        // 私有字段：本不应被外部直接设置
        @SuppressWarnings("unused")
        private String command;

        public void bindField(String name, String value) throws Exception {
            Field f = CommandTarget.class.getDeclaredField(name);
            f.setAccessible(true); // 写私有字段
            f.set(this, value);
        }

        public void execute() throws Exception {
            
            Runtime.getRuntime().exec(command); // 仅占位，不连真实远端
        }
    }

    


    public static class EngineTarget {
        // 私有字段：脚本引擎引用（被注入后惰性求值触发 eval）
        @SuppressWarnings("unused")
        private ScriptEngine engine;

        public void bindField(String name, Object value) throws Exception {
            Field f = EngineTarget.class.getDeclaredField(name);
            f.setAccessible(true); // 写入私有字段
            f.set(this, value);
        }

        public Object evalLazy() throws Exception {
            
            return engine.eval("localhost-demo"); // 仅占位，不连真实远端
        }
    }

    


    public void bind(Map<String, String> fields) throws Exception {
        CommandTarget target = new CommandTarget();
        for (Map.Entry<String, String> e : fields.entrySet()) {
            
            target.bindField(e.getKey(), e.getValue());
        }
        /*ANCHOR_1*/
        target.execute(); // 读私有字段 command -> Runtime.exec
    }

    


    public void bindEngine(Map<String, Object> fields) throws Exception {
        EngineTarget target = new EngineTarget();
        ScriptEngineManager manager = new ScriptEngineManager();
        for (Map.Entry<String, Object> e : fields.entrySet()) {
            Object val = e.getValue();
            if (val == null && "engine".equals(e.getKey())) {
                val = manager.getEngineByName("js"); // 注入脚本引擎
            }
            
            target.bindField(e.getKey(), val); // 行：私有字段写入
        }
        /*ANCHOR_2*/
        target.evalLazy(); // 行：eval 触发（读私有 engine 字段）
    }
}
