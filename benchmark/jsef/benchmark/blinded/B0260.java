package blinded;

import javax.script.ScriptEngine;
import javax.script.ScriptEngineManager;
import javax.script.ScriptException;
import java.util.Arrays;
import java.util.List;













public class ScriptEngineSandboxEscapeBy {

    
    private static final List<String> SANDBOX_ALLOWED = Arrays.asList("toLowerCase", "toUpperCase", "trim");

    




    public static Object runInSandboxBy(String untrustedScript) throws ScriptException {
        ScriptEngineManager manager = new ScriptEngineManager();
        ScriptEngine engine = manager.getEngineByName("js");

        // 安全策略：仅暴露白名单方法，不暴露反射/危险类绑定
        /*ANCHOR_1*/
        return engine.eval(untrustedScript);  // 受限环境：反射/Runtime 不可达
    }

    




    public static Object reflectContained(Object sandboxBoundValue, String command) {
        // 安全策略：拒绝任何反射调用，command 不参与链路
        if (!SANDBOX_ALLOWED.contains(String.valueOf(command))) {
            throw new SecurityException("reflection/unby call denied by sandbox policy");
        }
        /*ANCHOR_2*/
        return sandboxBoundValue.toString();  // 仅无害字符串操作，无反射 exec
    }

    public static void main(String[] args) throws Exception {
        reflectContained("localhost-demo", "trim");
    }
}
