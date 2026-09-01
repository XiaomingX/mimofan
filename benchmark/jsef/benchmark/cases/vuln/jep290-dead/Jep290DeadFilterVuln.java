package com.jsef.benchmark.vuln.jep290dead;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.io.ObjectInputFilter;
import java.io.ObjectInputStream;
import java.util.logging.Logger;

/**
 * JSEF-Benchmark L3 — 名存实亡的 JEP290 过滤器（CWE-502）
 *
 * 语义：ObjectInputStream 上 setObjectInputFilter 挂了"过滤器"，
 * 但该 filter 只打日志，对任何类都返回 ObjectInputFilter.Status.UNDECIDED。
 * UNDECIDED 表示"本过滤器不表态"——按 JDK 默认过滤策略，所有类一律放行，
 * 随后 readObject() 直接触发 gadget。
 *
 * 危险性：静态看代码"有过滤器"极易误判为安全；实际上过滤器形同虚设。
 *
 * CWE-502 (Deserialization of Untrusted Data)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用脚本。
 *
 * 修复要点（对照 Jep290DeadFilterSafe.java）：白名单 filter，
 * 危险包直接 REJECTED，其余按白名单 ALLOWED/REJECTED。
 */
public class Jep290DeadFilterVuln {

    private static final Logger LOG = Logger.getLogger(Jep290DeadFilterVuln.class.getName());

    public Object read(byte[] payload) throws IOException, ClassNotFoundException {
        ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(payload));
        // 节点1：看似挂了 JEP290 过滤器
        ois.setObjectInputFilter(Jep290DeadFilterVuln::logOnlyFilter);
        // [VULN] 漏洞点：过滤器只记日志、全部 UNDECIDED 放行，随后 readObject 触发 gadget
        // [CHECKPOINT id=JSEF-JEP290-001 cwe=502 level=L3 source=serialized payload sink=ObjectInputFilter only logs / returns UNDECIDED then readObject expect=VULN trace=benchmark/cases/vuln/jep290-dead/Jep290DeadFilterVuln.java:32,benchmark/cases/vuln/jep290-dead/Jep290DeadFilterVuln.java:41,benchmark/cases/vuln/jep290-dead/Jep290DeadFilterVuln.java:35]
        return ois.readObject();
    }

    /** 节点2：过滤器仅记日志，任何类都返回 UNDECIDED —— 形同虚设 */
    private static ObjectInputFilter.Status logOnlyFilter(ObjectInputFilter.FilterInfo info) {
        LOG.info("filtering class: " + info.serialClass());
        return ObjectInputFilter.Status.UNDECIDED;
    }
}
