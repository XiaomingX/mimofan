package blinded;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.io.ObjectInputFilter;
import java.io.ObjectInputStream;
import java.util.logging.Logger;

















public class Jep290DeadFilterBx {

    private static final Logger LOG = Logger.getLogger(Jep290DeadFilterBx.class.getName());

    public Object read(byte[] payload) throws IOException, ClassNotFoundException {
        ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(payload));
        // 节点1：看似挂了 JEP290 过滤器
        ois.setObjectInputFilter(Jep290DeadFilterBx::logOnlyFilter);
        
        /*ANCHOR_1*/
        return ois.readObject();
    }

    
    private static ObjectInputFilter.Status logOnlyFilter(ObjectInputFilter.FilterInfo info) {
        LOG.info("filtering class: " + info.serialClass());
        return ObjectInputFilter.Status.UNDECIDED;
    }
}
