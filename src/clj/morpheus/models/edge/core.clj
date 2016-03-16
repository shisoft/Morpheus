(ns morpheus.models.edge.core
  (:require [morpheus.utils :refer :all]
            [morpheus.models.edge.directed]
            [morpheus.models.edge.indirected]
            [morpheus.models.edge.hyper]
            [morpheus.models.edge.simple]
            [morpheus.models.edge.defined]
            [morpheus.models.edge.dynamic]
            [morpheus.models.edge.base :as eb]
            [morpheus.models.vertex.core :as v]
            [neb.core :as neb]
            [morpheus.models.core :as core]
            [cluster-connector.utils.for-debug :refer [$ spy]]
            [neb.utils :refer [map-on-vals]]
            [morpheus.models.base :as mb]))

(defn new-edge-group [group-name group-props]
  (let [{:keys [fields]} group-props
        require-edge-cell?  (eb/require-edge-cell? group-props)
        base-schema      (eb/edge-base-schema group-props)
        fields (when require-edge-cell? (eb/edge-schema group-props base-schema fields))]
    (core/add-schema :e group-name fields group-props)))

(defn edge-group-props [group] (core/get-schema :e group))

(defn create-edge [v1 group v2 & args]
  (let [[v1-id v2-id] (map :*id* [v1 v2])
        edge-schema (edge-group-props group)
        edge-schema-id (:id edge-schema)
        [v1-v-field v2-v-field
         type-body-sticker] ((juxt eb/v1-vertex-field
                                   eb/v2-vertex-field
                                   eb/type-stick-body) edge-schema)
        require-edge-cell? (eb/require-edge-cell? edge-schema)
        edge-cell-vertex-fields (eb/edge-cell-vertex-fields edge-schema v1-id v2-id)]
    (when type-body-sticker (assert (= type-body-sticker (:body edge-schema))
                                    (str type-body-sticker " cannot with body type " (:body edge-schema))))
    (let [edge-cell-id (when require-edge-cell? (apply eb/create-edge-cell
                                                       edge-schema
                                                       edge-cell-vertex-fields args))]
      (neb/update-cell* v1-id 'morpheus.models.edge.base/record-edge-on-vertex
                        edge-schema-id v1-v-field (or edge-cell-id v2-id))
      (neb/update-cell* v2-id 'morpheus.models.edge.base/record-edge-on-vertex
                        edge-schema-id v2-v-field (or edge-cell-id v1-id)))))

(defn neighbours [vertex & {:keys [directions edge-groups]}]
  (let [vertex-id (:*id* vertex)
        direction-fields (set (or (when directions
                                    (if (vector? directions)
                                      directions [directions]))
                                  [:*inbounds* :*outbounds* :*neighbours*]))
        edge-groups (when edge-groups
                      (into #{}
                            (map
                              (partial core/get-schema-id :e)
                              (if (vector? edge-groups)
                                edge-groups [edge-groups]))))
        cid-lists (select-keys vertex direction-fields)
        cid-lists (->> (map
                         (fn [[direction dir-cid-list]]
                           (when (direction-fields direction)
                             (map
                               (fn [{:keys [sid list-cid]}]
                                 (when (or (nil? edge-groups)
                                           (edge-groups sid))
                                   (assoc (select-keys (neb/read-cell* list-cid)
                                                       [:cid-array])
                                     :direction direction
                                     :group-props (mb/schema-by-id sid))))
                               dir-cid-list)))
                         cid-lists)
                       (flatten)
                       (filter identity))]
    (map
      (fn [{:keys [group-props] :as cid-list}]
        (let [vertex-fields (eb/vertex-fields group-props)]
          (merge
            {:edges
             (map
               (fn [edge]
                 (let [pure-edge (dissoc edge :*schema* :*hash*)]
                   (into {} (map
                              (fn [[k v]]
                                [k (if (vertex-fields k)
                                     (delay
                                       (v/get-veterx-by-id v))
                                     v)])
                              pure-edge))))
               (eb/edges-from-cid-array group-props cid-list vertex-id))}
            (select-keys group-props [:name :type])
            (select-keys cid-list [:direction]))))
      cid-lists)))