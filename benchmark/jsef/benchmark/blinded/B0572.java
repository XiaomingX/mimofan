package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;

























@RestController
public class Entry {

    private final Config config = new Config();
    private final SpelParser spelParser = new SpelParser();

    @PostMapping("/benchmark/longrange/spel/unby")
    public String handle(@RequestBody String requestBody) {
        // 入口：不可信请求体进入链路
        Config.AppConfig cfg = config.loadConfig(requestBody);     // 传递点 2（见 Config.java:39）
        String expr = cfg.getExpression();                         // 传递点 3（见 Config.java:21）
        // 暴露内部方法的 root 对象（语义桩：真实库可能暴露 T() 可达类）
        BeanDefinitionRoot root = new BeanDefinitionRoot();
        Object evaluated = spelParser.parseAndEvaluate(expr, root); // 传递点 4-5（见 SpelParser.java:38,40）

        /*ANCHOR_1*/
        return registerBean(String.valueOf(evaluated)); // 污点拼入"可执行上下文"（bean 定义/查询）
    }

    
    static String registerBean(String value) {
        // 语义等价：DefaultListableBeanFactory.registerBeanDefinition(...)
        //          或 JpaRepository 动态查询拼接
        System.out.println("[bean-register] " + value);
        return "registered:" + value;
    }

    
    static class BeanDefinitionRoot {
        public String getName() {
            return "app";
        }
    }
}
