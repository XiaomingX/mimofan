
package blinded;

import javax.xml.transform.Transformer;
import javax.xml.transform.TransformerFactory;
import javax.xml.transform.stream.StreamSource;
import java.io.StringReader;





















public class XsltInjection {

    


    static String getStylesheet(String userInput) {
        return userInput; // 不可信 XSLT 内容
    }

    


    static Transformer build(String userInput) throws Exception {
        String sheet = getStylesheet(userInput);
        TransformerFactory factory = TransformerFactory.newInstance();
        /*ANCHOR_1*/
        Transformer transformer = factory.newTransformer(new StreamSource(new StringReader(sheet)));
        return transformer;
    }
}
