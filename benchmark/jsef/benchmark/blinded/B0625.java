
package blinded;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.DefaultTyping;
















public class PatchDeserializeBx {

    


    static Object fromJson(String json) throws Exception {
        ObjectMapper mapper = new ObjectMapper();
        mapper.activateDefaultTyping(
            mapper.getPolymorphicTypeValidator(), DefaultTyping.EVERYTHING); // 危险：全类型
        /*ANCHOR_1*/
        return mapper.readValue(json, Object.class); // @class 可指定任意类 -> gadget 可达
    }
}
