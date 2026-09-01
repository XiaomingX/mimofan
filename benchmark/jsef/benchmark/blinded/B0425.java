
package blinded;







public class RequestContext {

    private static final ThreadLocal<String> PRINCIPAL = new ThreadLocal<>();

    public void setPrincipal(String name) {
        // 注入当前身份（认证边界建立点）
        /*ANCHOR_1*/
        PRINCIPAL.set(name);
    }

    public String getPrincipal() {
        return PRINCIPAL.get();
    }
}
