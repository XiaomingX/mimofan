
package blinded;

import javax.xml.transform.Transformer;
import javax.xml.transform.TransformerFactory;
import javax.xml.transform.stream.StreamSource;
import java.io.StringReader;


















public class XsltInjection_By {

    // 受信模板：编译期固定，不由用户输入决定
    private static final String TRUSTED_XSLT =
        "<xsl:stylesheet version='1.0' xmlns:xsl='http://www.w3.org/1999/XSL/Transform'>"
        + "<xsl:template match='/'><root/></xsl:template></xsl:stylesheet>";

    


    static Transformer build() throws Exception {
        TransformerFactory factory = TransformerFactory.newInstance();
        // 禁用外部实体 / 外部样式表 / 脚本访问
        try {
            factory.setAttribute("http://javax.xml.XMLConstants/property/accessExternalDTD", "");
            factory.setAttribute("http://javax.xml.XMLConstants/property/accessExternalStylesheet", "");
        } catch (Exception ignored) { /* 部分实现不支持属性名，靠 SECURE_PROCESSING */ }
        factory.setFeature(javax.xml.XMLConstants.FEATURE_SECURE_PROCESSING, true);
        /*ANCHOR_1*/
        Transformer transformer = factory.newTransformer(new StreamSource(new StringReader(TRUSTED_XSLT)));
        return transformer;
    }
}
