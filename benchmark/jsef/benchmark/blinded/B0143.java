
// 安全对照：JNDI 注入（修复版）
// 修复原则：禁止用户可控输入参与 JNDI lookup；使用白名单限制允许访问的资源名/协议，
//          仅从可信固定地址获取资源。本示例使用固定受信任资源名，拒绝任意用户输入。
package blinded;

import org.springframework.web.bind.annotation.*;
import javax.naming.Context;
import javax.naming.InitialContext;
import javax.naming.NamingException;




@RestController
@RequestMapping("/benchmark/bx/jndi")
public class JndiInjectionBy {

    // 受信任的固定资源名（非用户输入）
    private static final String TRUSTED_RESOURCE = "java:comp/env/jdbc/AppDB";

    


    @GetMapping("/by/rmi-jndi-lookup")
    public String byRmiJndiLookup(@RequestParam String jndiResourceName) throws NamingException {
        // 安全实践：拒绝任意用户输入，仅允许白名单内的受信任资源。
        if (!TRUSTED_RESOURCE.equals(jndiResourceName)) {
            return "{'msg':'拒绝访问：资源不在受信任白名单中'}";
        }
        /*ANCHOR_1*/
        InitialContext initialContext = new InitialContext();
        initialContext.lookup(TRUSTED_RESOURCE);
        return "{'msg':'by'}";
    }

    


    @PostMapping("/by/dynamic-jndi-url-build")
    public String byDynamicJndiUrlBuild(@RequestBody String userInput) throws NamingException {
        // 安全实践：用户输入不再参与 lookup 地址拼接。
        /*ANCHOR_2*/
        return "{'msg':'已拒绝使用不可信输入构造 lookup 地址（安全）'}";
    }
}
