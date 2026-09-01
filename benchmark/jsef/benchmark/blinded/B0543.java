package blinded;

// 运行态需 JSEF 依赖：本文件为自包含 benchmark 样本，使用 Spring 注解仅为演示
// 跨文件调用链语义（CAP-07 跨编译单元）。实际运行需 Spring Web 依赖，此处不强求编译。

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;














@RestController
public class ChainSqlController {

    private final ChainSqlService service;

    public ChainSqlController(ChainSqlService service) {
        this.service = service;
    }

    @GetMapping("/benchmark/chainsql/unby")
    public String handle(@RequestParam String input) {
        /*ANCHOR_1*/
        return service.process(input); // 污点 input 跨编译单元流向 ChainSqlService -> ChainSqlMapper
    }
}
