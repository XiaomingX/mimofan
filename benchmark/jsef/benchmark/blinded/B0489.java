package blinded;

import com.alibaba.fastjson.JSON;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;










@RestController
public class FastjsonAutoTypeOn {

    @PostMapping("/api/v1/fastjson/unby/parse")
    public Object parse(@RequestBody String untrustedJson) {
        com.alibaba.fastjson.parser.ParserConfig.getGlobalInstance().setAutoTypeSupport(true);
        /*ANCHOR_1*/
        return JSON.parseObject(untrustedJson); // autotype 开启，可实例化任意 @type
    }
}
