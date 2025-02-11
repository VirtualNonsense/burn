use crate::{
    backends::cpu, ConnectedStatsOptions, ConnectedStatsPrimitive, Connectivity, VisionOps,
};
#[cfg(feature = "fusion")]
use burn_fusion::{client::FusionClient, stream::Operation, Fusion, FusionBackend, FusionRuntime};
use burn_jit::{BoolElement, FloatElement, IntElement, JitBackend, JitRuntime};
use burn_tensor::ops::{BoolTensor, IntTensor};
#[cfg(feature = "fusion")]
use burn_tensor::{
    repr::{CustomOpDescription, HandleContainer, OperationDescription},
    Element,
};

use super::connected_components::hardware_accelerated;

impl<R, F, I, BT> VisionOps<Self> for JitBackend<R, F, I, BT>
where
    R: JitRuntime,
    F: FloatElement,
    I: IntElement,
    BT: BoolElement,
{
    fn connected_components(img: BoolTensor<Self>, connectivity: Connectivity) -> IntTensor<Self> {
        hardware_accelerated::<R, F, I, BT>(
            img.clone(),
            ConnectedStatsOptions::none(),
            connectivity,
        )
        .map(|it| it.0)
        .unwrap_or_else(|_| cpu::connected_components::<Self>(img, connectivity))
    }

    fn connected_components_with_stats(
        img: BoolTensor<Self>,
        connectivity: Connectivity,
        opts: ConnectedStatsOptions,
    ) -> (IntTensor<Self>, ConnectedStatsPrimitive<Self>) {
        hardware_accelerated::<R, F, I, BT>(img.clone(), opts, connectivity).unwrap_or_else(|_| {
            cpu::connected_components_with_stats::<Self>(img, connectivity, opts)
        })
    }
}

#[cfg(feature = "fusion")]
impl<B: FusionBackend + VisionOps<B>> VisionOps<Self> for Fusion<B> {
    fn connected_components(img: BoolTensor<Self>, conn: Connectivity) -> IntTensor<Self> {
        let batches = img.shape[0];
        let height = img.shape[1];
        let width = img.shape[2];
        let client = img.client.clone();

        #[derive(derive_new::new)]
        struct ConnComp<B> {
            desc: CustomOpDescription,
            conn: Connectivity,
            _b: core::marker::PhantomData<B>,
        }

        impl<B1: FusionBackend + VisionOps<B1>> Operation<B1::FusionRuntime> for ConnComp<B1> {
            fn execute(
                self: Box<Self>,
                handles: &mut HandleContainer<<B1::FusionRuntime as FusionRuntime>::FusionHandle>,
            ) {
                let ([img], [labels]) = self.desc.consume();
                let input = handles.get_bool_tensor::<B1>(&img);
                let output = B1::connected_components(input, self.conn);

                handles.register_int_tensor::<B1>(&labels.id, output);
            }
        }

        let stream = img.stream;
        let out = client.tensor_uninitialized(vec![batches, height, width], B::IntElem::dtype());

        let desc = CustomOpDescription::new(
            "connected_components",
            &[img.into_description()],
            &[out.to_description_out()],
        );
        client.register(
            vec![stream],
            OperationDescription::Custom(desc.clone()),
            ConnComp::<B>::new(desc, conn),
        );

        out
    }

    fn connected_components_with_stats(
        img: BoolTensor<Self>,
        conn: Connectivity,
        opts: ConnectedStatsOptions,
    ) -> (IntTensor<Self>, ConnectedStatsPrimitive<Self>) {
        let batches = img.shape[0];
        let height = img.shape[1];
        let width = img.shape[2];
        let client = img.client.clone();

        #[derive(derive_new::new)]
        struct ConnCompStats<B> {
            desc: CustomOpDescription,
            conn: Connectivity,
            opts: ConnectedStatsOptions,
            _b: core::marker::PhantomData<B>,
        }

        impl<B1: FusionBackend + VisionOps<B1>> Operation<B1::FusionRuntime> for ConnCompStats<B1> {
            fn execute(
                self: Box<Self>,
                handles: &mut HandleContainer<<B1::FusionRuntime as FusionRuntime>::FusionHandle>,
            ) {
                let ([img], [labels, area, left, top, right, bottom, max_label]) =
                    self.desc.consume();
                let input = handles.get_bool_tensor::<B1>(&img);
                let (output, stats) =
                    B1::connected_components_with_stats(input, self.conn, self.opts);

                handles.register_int_tensor::<B1>(&labels.id, output);
                handles.register_int_tensor::<B1>(&area.id, stats.area);
                handles.register_int_tensor::<B1>(&left.id, stats.left);
                handles.register_int_tensor::<B1>(&top.id, stats.top);
                handles.register_int_tensor::<B1>(&right.id, stats.right);
                handles.register_int_tensor::<B1>(&bottom.id, stats.bottom);
                handles.register_int_tensor::<B1>(&max_label.id, stats.max_label);
            }
        }

        let stream = img.stream;
        let out = client.tensor_uninitialized(vec![batches, height, width], B::IntElem::dtype());
        let area = client.tensor_uninitialized(vec![batches, height * width], B::IntElem::dtype());
        let left = client.tensor_uninitialized(vec![batches, height * width], B::IntElem::dtype());
        let top = client.tensor_uninitialized(vec![batches, height * width], B::IntElem::dtype());
        let right = client.tensor_uninitialized(vec![batches, height * width], B::IntElem::dtype());
        let bottom =
            client.tensor_uninitialized(vec![batches, height * width], B::IntElem::dtype());
        let max_label = client.tensor_uninitialized(vec![batches], B::IntElem::dtype());

        let desc = CustomOpDescription::new(
            "connected_components",
            &[img.into_description()],
            &[
                out.to_description_out(),
                area.to_description_out(),
                left.to_description_out(),
                top.to_description_out(),
                right.to_description_out(),
                bottom.to_description_out(),
                max_label.to_description_out(),
            ],
        );
        client.register(
            vec![stream],
            OperationDescription::Custom(desc.clone()),
            ConnCompStats::<B>::new(desc, conn, opts),
        );

        let stats = ConnectedStatsPrimitive {
            area,
            left,
            top,
            right,
            bottom,
            max_label,
        };
        (out, stats)
    }
}
