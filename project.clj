(defproject morpheus "0.1.0-SNAPSHOT"
  :description "FIXME: write description"
  :url "http://example.com/FIXME"
  :license {:name "Eclipse Public License"
            :url "http://www.eclipse.org/legal/epl-v10.html"}
  :dependencies [[org.clojure/clojure "1.7.0"]
                 [neb "0.1.0-SNAPSHOT"]]
  :source-paths ["src/clj"]
  :java-source-paths ["src/java"]
  :main ^:skip-aot morpheus.core
  :target-path "target/%s"
  :plugins [[lein-midje "3.1.3"]]
  :profiles {:uberjar {:aot :all}
             :dev {:dependencies [[midje "1.8.3"]]}}
  :jvm-opts [;"-Djava.rmi.server.hostname=<hostname>" ;;add this when remote-connect fail
             ;"-Dcom.sun.management.jmxremote"
             ;"-Dcom.sun.management.jmxremote.port=9876"
             ;"-Dcom.sun.management.jmxremote.authenticate=false"
             ;"-Dcom.sun.management.jmxremote.ssl=false"
             "-Xmx8g"
             "-XX:+UseParNewGC" "-XX:+UseConcMarkSweepGC" "-XX:+CMSParallelRemarkEnabled"])
