package blinded;

import com.alibaba.fastjson.JSON;
import com.alibaba.fastjson.parser.ParserConfig;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;

import java.util.Map;




@RestController
public class FastjsonAutoTypeOnBy {

    @PostMapping("/api/v1/fastjson/by/parse")
    public Map<String, Object> parse(@RequestBody String untrustedJson) {
        ParserConfig.getGlobalInstance().setAutoTypeSupport(false); // 关闭 autotype
        /*ANCHOR_1*/
        return JSON.parseObject(untrustedJson, Map.class); // 仅映射到已知类型
    }
}
