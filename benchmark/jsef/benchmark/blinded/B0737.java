package blinded;

import org.springframework.web.util.HtmlUtils;





















public class XssJsContextMismatchBx {

    




    public String render(String user) {
        String escaped = HtmlUtils.htmlEscape(user); // 节点 1：HTML 上下文转义（对 JS 字符串无效）
        String js = "var name = '" + escaped + "';"; // 节点 2：拼入 <script> 单引号字符串
        /*ANCHOR_1*/
        return "<script>" + js + "</script>"; 
    }
}
