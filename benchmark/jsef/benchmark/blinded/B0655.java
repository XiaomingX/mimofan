package blinded;

import java.lang.reflect.Method;
import javax.script.ScriptEngine;
import javax.script.ScriptEngineManager;
import javax.script.ScriptException;
















public class ScriptEngineSandboxEscape {

    




    public static Object runInSandbox(String untrustedScript) throws ScriptException {
        ScriptEngineManager manager = new ScriptEngineManager();
        ScriptEngine engine = manager.getEngineByName("js");

        // 沙箱"看似受限"：仅暴露有限绑定对象，未禁止反射
        // 危险 sink：脚本内部可通过反射拿到 Runtime，逃逸沙箱约束
        /*ANCHOR_1*/
        return engine.eval(untrustedScript);
    }

    





    public static Object reflectOutOfSandbox(Object sandboxBoundValue, String command) {
        try {
            // 沙箱约束被反射绕过：从任意对象出发拿到 java.lang.Runtime
            Method getClass = Object.class.getMethod("getClass");
            Class<?> runtimeClass = Class.forName("java.lang.Runtime");
            Method getRuntime = runtimeClass.getMethod("getRuntime");
            Object runtime = getRuntime.invoke(null);

            // 危险 sink：反射驱动 Runtime.exec —— 沙箱限制被突破
            /*ANCHOR_2*/
            Method exec = runtimeClass.getMethod("exec", String.class);
            return exec.invoke(runtime, command);
        } catch (Exception e) {
            throw new RuntimeException("sandbox reflection escape (demo only)", e);
        }
    }

    public static void main(String[] args) throws Exception {
        // 仅演示逃逸可达性语义，command 为 localhost 演示占位，不连接真实目标
        reflectOutOfSandbox("localhost-demo", "echo localhost-demo");
    }
}
